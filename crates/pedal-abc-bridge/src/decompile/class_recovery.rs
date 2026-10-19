//! 从定义指令与词法环境恢复 record 内的类和方法归属。
//! @author ljlVink

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use super::{NestedStyle, cfg, lift, printer, structure};
use crate::disasm::Ins;
use crate::file::{AbcFile, ClassInfo, LiteralValue, MethodInfo, ModuleRecord};
use lift::{Expr, LiftedBody, Stmt};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Unknown,
    Class(usize),
    Prototype(usize),
    Function(u32),
    Name(String),
    String(String),
    Empty,
    Receiver(u32),
}

#[derive(Clone)]
struct Member {
    name: String,
    method: u32,
    static_: bool,
    accessor: Option<&'static str>,
}

struct RecoveredClass {
    owner: u32,
    name: String,
    constructor: u32,
    buffer: u32,
    parent: Option<String>,
    members: Vec<Member>,
    initializers: Vec<u32>,
    sendable: bool,
}

#[derive(Default)]
struct Frame {
    owner: u32,
    names: Vec<String>,
    values: HashMap<u32, Value>,
    entry: bool,
}

/// 元数据来自真实 defineclass/definemethod 指令，绝不按同名方法猜测归属。
#[derive(Default)]
pub(crate) struct RecordRecovery {
    classes: Vec<RecoveredClass>,
    frames: Vec<Frame>,
    bindings: HashMap<u32, HashMap<(u32, u32), Option<usize>>>,
    /// 记录各指令经正常与异常控制流证明一致的环境身份，区分复用的 depth/slot。
    entry_bindings: HashMap<u32, HashMap<usize, (usize, u32)>>,
    member_methods: HashSet<u32>,
    embedded_methods: HashSet<u32>,
    functions: HashMap<u32, String>,
    main: u32,
    record: u32,
    accessors_complete: bool,
    /// 已证实只含数据字段的私有键；使用 WeakMap 保持独立身份与品牌检查。
    private_keys: HashMap<(usize, u32), String>,
    /// 箭头闭包打印时继承创建点的 this；冲突创建点停止推断。
    receiver_owners: HashMap<u32, Option<u32>>,
}

fn operands(ins: &Ins, prefix: &str) -> Vec<u32> {
    ins.sig_values()
        .into_iter()
        .filter(|(n, _, _)| n.starts_with(prefix))
        .map(|(_, _, v)| v as u32)
        .collect()
}

fn resolved(file: &AbcFile, method: u32, ins: &Ins, prefix: &str) -> Option<u32> {
    file.resolve_msl(method, *operands(ins, prefix).first()?)
        .ok()
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && !matches!(
            name,
            "this"
                | "super"
                | "class"
                | "function"
                | "return"
                | "let"
                | "const"
                | "var"
                | "new"
                | "default"
                | "await"
                | "yield"
                | "null"
                | "true"
                | "false"
        )
        && name.chars().enumerate().all(|(i, c)| {
            c == '_' || c == '$' || c.is_alphabetic() || (i > 0 && c.is_ascii_digit())
        })
}

/// lde 外部变量索引指向导入绑定表；返回可直接用于表达式的本地绑定名。
fn external_import_name(module: Option<&ModuleRecord>, index: u64) -> Option<String> {
    let module = module?;
    module
        .regular_imports
        .get(index as usize)
        .map(|import| import.local_name.as_str())
        .or_else(|| {
            module
                .namespace_imports
                .get(index as usize)
                .map(|import| import.local_name.as_str())
        })
        .filter(|name| identifier(name))
        .map(str::to_owned)
}

/// getmodulenamespace 的立即数是 module request 索引，按 module_idx 匹配，不能当 namespace 表下标。
fn namespace_import_name(module: Option<&ModuleRecord>, request: u64) -> Option<String> {
    let module = module?;
    let mut matches = module
        .namespace_imports
        .iter()
        .filter(|import| u64::from(import.module_idx) == request)
        .map(|import| import.local_name.as_str());
    let name = matches.next()?;
    if matches.next().is_some() || !identifier(name) {
        return None;
    }
    Some(name.to_owned())
}

/// 沿正常与异常控制流传播词法环境栈；合流处不一致即停止猜测捕获绑定。
fn lexical_environments(
    ins: &[Ins],
    graph: &cfg::Cfg,
    inherited: &[usize],
    frames: &HashMap<usize, usize>,
) -> HashMap<usize, Vec<usize>> {
    if graph.blocks.is_empty() {
        return HashMap::new();
    }
    let mut entries = HashMap::from([(0, Some(inherited.to_vec()))]);
    let mut pending = VecDeque::from([0]);
    fn merge(
        entries: &mut HashMap<usize, Option<Vec<usize>>>,
        pending: &mut VecDeque<usize>,
        successor: usize,
        state: &Option<Vec<usize>>,
    ) {
        match entries.entry(successor) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(state.clone());
                pending.push_back(successor);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get().is_some() && entry.get() != state {
                    entry.insert(None);
                    pending.push_back(successor);
                }
            }
        }
    }
    while let Some(block) = pending.pop_front() {
        let mut state = entries[&block].clone();
        for instruction in &ins[graph.blocks[block].start..graph.blocks[block].end] {
            // 每条指令都可能是异常入口来源。保护区内若有环境栈变化，
            // handler 合流自动变为未知，不推测 VM 的环境回退行为。
            for (_, _, handler_pc) in &graph.blocks[block].try_of {
                if let Some(&handler) = graph.pc_to_block.get(&(*handler_pc as usize)) {
                    merge(&mut entries, &mut pending, handler, &state);
                }
            }
            if let Some(env) = &mut state {
                if let Some(frame) = frames.get(&instruction.pc) {
                    env.push(*frame);
                } else if instruction.meta.mnemonic == "poplexenv" && env.pop().is_none() {
                    state = None;
                }
            }
        }
        for &successor in &graph.blocks[block].succs {
            merge(&mut entries, &mut pending, successor, &state);
        }
    }
    let mut result = HashMap::new();
    for (block, state) in entries {
        let Some(mut env) = state else { continue };
        for instruction in &ins[graph.blocks[block].start..graph.blocks[block].end] {
            result.insert(instruction.pc, env.clone());
            if let Some(frame) = frames.get(&instruction.pc) {
                env.push(*frame);
            } else if instruction.meta.mnemonic == "poplexenv" {
                env.pop();
            }
        }
    }
    result
}

/// 分配环境的块经正常或异常边可再次到达时，不能提升为函数作用域的一份绑定。
fn environment_repeats(graph: &cfg::Cfg, pc: usize) -> bool {
    let Some(site) = graph
        .blocks
        .iter()
        .position(|block| block.start_pc <= pc && pc < block.end_pc)
    else {
        return true;
    };
    let successors = |block: usize| {
        graph.blocks[block].succs.iter().copied().chain(
            graph.blocks[block]
                .try_of
                .iter()
                .filter_map(|(_, _, handler)| graph.pc_to_block.get(&(*handler as usize)).copied()),
        )
    };
    let mut pending: Vec<_> = successors(site).collect();
    let mut visited = HashSet::new();
    while let Some(block) = pending.pop() {
        if block == site {
            return true;
        }
        if visited.insert(block) {
            pending.extend(successors(block));
        }
    }
    false
}

impl RecordRecovery {
    pub(crate) fn build(file: &AbcFile, record: &ClassInfo, module: Option<&ModuleRecord>) -> Self {
        let mut out = Self {
            record: record.offset,
            ..Self::default()
        };
        if file.header.kind != crate::file::HeaderKind::Modern {
            return out;
        }
        let Some(main) = record.methods.iter().find(|m| m.name == "func_main_0") else {
            return out;
        };
        if main.code.as_ref().is_none_or(|code| {
            !crate::disasm::decode(code.instructions(file.bytes()))
                .is_ok_and(|ins| ins.len() <= 2_000)
        }) {
            return out;
        }
        out.main = main.offset;
        out.record = record.offset;
        out.accessors_complete = true;
        let mut queue = VecDeque::from([(main.offset, Vec::new())]);
        let mut scanned = HashSet::new();
        while let Some((off, inherited)) = queue.pop_front() {
            if !scanned.insert(off) {
                continue;
            }
            let Ok(method) = file.method_at(off) else {
                continue;
            };
            out.scan(file, &method, inherited, module, &mut queue);
        }
        for class in &out.classes {
            out.member_methods.insert(class.constructor);
            out.member_methods
                .extend(class.members.iter().map(|m| m.method));
            out.member_methods.extend(&class.initializers);
        }
        out
    }

    pub(crate) fn has_classes(&self) -> bool {
        !self.classes.is_empty()
    }
    pub(crate) fn is_arkui_constructor(&self, offset: u32) -> bool {
        self.classes.iter().any(|class| {
            class.constructor == offset
                && matches!(
                    class.parent.as_deref(),
                    Some("globalThis.ViewPU" | "ViewPU")
                )
        })
    }
    pub(crate) fn is_member(&self, off: u32) -> bool {
        self.member_methods.contains(&off) || self.embedded_methods.contains(&off)
    }
    pub(crate) fn module(&self, file: &AbcFile) -> Option<ModuleRecord> {
        super::module_record_of(file, file.classes.iter().find(|c| c.offset == self.record)?)
    }

    fn scan(
        &mut self,
        file: &AbcFile,
        method: &MethodInfo,
        mut env: Vec<usize>,
        module: Option<&ModuleRecord>,
        queue: &mut VecDeque<(u32, Vec<usize>)>,
    ) {
        let Some(code) = &method.code else { return };
        if code.code_size > 20_000 {
            return;
        }
        let Ok(ins) = crate::disasm::decode_with(
            code.instructions(file.bytes()),
            file.header.kind == crate::file::HeaderKind::Legacy,
        ) else {
            return;
        };
        if ins.len() > 4_000 {
            return;
        }
        let graph = cfg::build(&ins, code.code_size as usize, &code.try_blocks);
        let mut frame_at = HashMap::new();
        for instruction in &ins {
            if matches!(
                instruction.meta.mnemonic,
                "newlexenv" | "wide.newlexenv" | "newlexenvwithname" | "wide.newlexenvwithname"
            ) {
                let names = resolved(file, method.offset, instruction, "literalarray_id")
                    .and_then(|off| file.read_literal_array(off).ok())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|v| match v {
                        LiteralValue::String(off) => file.string_at(*off).ok(),
                        _ => None,
                    })
                    .collect();
                frame_at.insert(instruction.pc, self.frames.len());
                self.frames.push(Frame {
                    owner: method.offset,
                    names,
                    entry: !environment_repeats(&graph, instruction.pc),
                    ..Frame::default()
                });
            }
        }
        let environments = lexical_environments(&ins, &graph, &env, &frame_at);
        let mut reachable = HashSet::new();
        let handlers: HashSet<_> = code
            .try_blocks
            .iter()
            .flat_map(|region| &region.catch_blocks)
            .filter_map(|catch| graph.pc_to_block.get(&(catch.handler_pc as usize)).copied())
            .collect();
        let mut work = vec![0];
        while let Some(block) = work.pop() {
            if block < graph.blocks.len() && reachable.insert(block) {
                work.extend(&graph.blocks[block].succs);
                work.extend(
                    graph.blocks[block]
                        .try_of
                        .iter()
                        .filter_map(|(_, _, pc)| graph.pc_to_block.get(&(*pc as usize)).copied()),
                );
            }
        }
        let mut registers = HashMap::<u32, Value>::new();
        let mut acc = Value::Unknown;
        let mut active_class = None;
        let mut construction_methods = HashMap::new();
        for (bi, block) in graph.blocks.iter().enumerate() {
            if !reachable.contains(&bi) {
                continue;
            }
            let known_env = environments.get(&block.start_pc);
            env = known_env.cloned().unwrap_or_default();
            if graph.preds[bi].len() > 1 || handlers.contains(&bi) {
                registers.clear();
                acc = Value::Unknown;
                active_class = None;
            }
            for i in &ins[block.start..block.end] {
                let mn = i.meta.mnemonic;
                let regs = operands(i, "v");
                let imms = operands(i, "imm");
                let get = |n: usize| {
                    regs.get(n)
                        .map(|r| {
                            registers.get(r).cloned().unwrap_or_else(|| {
                                if code.num_args >= 3 && *r == code.num_vregs + 2 {
                                    Value::Receiver(method.offset)
                                } else {
                                    Value::Unknown
                                }
                            })
                        })
                        .unwrap_or(Value::Unknown)
                };
                let string = || {
                    resolved(file, method.offset, i, "string_id")
                        .and_then(|o| file.string_at(o).ok())
                };
                if matches!(
                    mn,
                    "ldprivateproperty" | "stprivateproperty" | "callruntime.defineprivateproperty"
                ) && let [_, depth, slot] = imms.as_slice()
                    && let Some(frame) = environments.get(&i.pc).and_then(|env| {
                        env.len()
                            .checked_sub(*depth as usize + 1)
                            .map(|index| env[index])
                    })
                    && self.private_keys.contains_key(&(frame, *slot))
                {
                    self.entry_bindings
                        .entry(method.offset)
                        .or_default()
                        .insert(i.pc, (frame, *slot));
                }
                match mn {
                    "newlexenv"
                    | "wide.newlexenv"
                    | "newlexenvwithname"
                    | "wide.newlexenvwithname" => {
                        env.push(frame_at[&i.pc]);
                    }
                    "poplexenv" => {
                        env.pop();
                    }
                    "stlexvar" | "wide.stlexvar" | "ldlexvar" | "wide.ldlexvar" => {
                        if let [depth, slot, ..] = imms.as_slice() {
                            let frame = environments.get(&i.pc).and_then(|env| {
                                env.len()
                                    .checked_sub(*depth as usize + 1)
                                    .map(|index| env[index])
                            });
                            let bindings = self.bindings.entry(method.offset).or_default();
                            bindings
                                .entry((*depth, *slot))
                                .and_modify(|old| {
                                    if *old != frame {
                                        *old = None
                                    }
                                })
                                .or_insert(frame);
                            if let Some(frame) = frame {
                                self.entry_bindings
                                    .entry(method.offset)
                                    .or_default()
                                    .insert(i.pc, (frame, *slot));
                                if mn.contains("stlexvar") {
                                    self.frames[frame]
                                        .values
                                        .entry(*slot)
                                        .and_modify(|old| {
                                            if *old != acc {
                                                *old = Value::Unknown;
                                            }
                                        })
                                        .or_insert_with(|| acc.clone());
                                } else {
                                    acc = self.frames[frame]
                                        .values
                                        .get(slot)
                                        .cloned()
                                        .unwrap_or(Value::Unknown);
                                }
                            } else if mn.contains("ldlexvar") {
                                acc = Value::Unknown;
                            }
                        }
                    }
                    "sta" => {
                        if let Some(r) = regs.first() {
                            registers.insert(*r, acc.clone());
                        }
                    }
                    "lda" => acc = get(0),
                    "ldthis" | "supercallthisrange" | "supercallarrowrange" | "supercallspread" => {
                        acc = Value::Receiver(method.offset)
                    }
                    "mov" => {
                        if let Some(r) = regs.first() {
                            let value = get(1);
                            registers.insert(*r, value);
                        }
                    }
                    "ldhole" | "ldundefined" | "ldnull" => acc = Value::Empty,
                    "lda.str" => acc = string().map(Value::String).unwrap_or(Value::Unknown),
                    "tryldglobalbyname" | "ldglobalvar" => {
                        acc = string()
                            .map(|s| Value::Name(format!("globalThis.{s}")))
                            .unwrap_or(Value::Unknown)
                    }
                    "ldlocalmodulevar"
                    | "wide.ldlocalmodulevar"
                    | "callruntime.ldsendablelocalmodulevar"
                    | "callruntime.wideldsendablelocalmodulevar" => {
                        acc = imms
                            .first()
                            .and_then(|idx| module?.local_exports.get(*idx as usize))
                            .map(|e| Value::Name(e.local_name.clone()))
                            .unwrap_or(Value::Unknown);
                    }
                    "ldexternalmodulevar"
                    | "wide.ldexternalmodulevar"
                    | "callruntime.ldlazymodulevar"
                    | "callruntime.wideldlazymodulevar"
                    | "callruntime.ldsendableexternalmodulevar"
                    | "callruntime.wideldsendableexternalmodulevar"
                    | "callruntime.ldlazysendablemodulevar"
                    | "callruntime.wideldlazysendablemodulevar" => {
                        acc = imms
                            .first()
                            .and_then(|idx| external_import_name(module, u64::from(*idx)))
                            .map(Value::Name)
                            .unwrap_or(Value::Unknown);
                    }
                    "getmodulenamespace" | "wide.getmodulenamespace" => {
                        acc = imms
                            .first()
                            .and_then(|idx| namespace_import_name(module, u64::from(*idx)))
                            .map(Value::Name)
                            .unwrap_or(Value::Unknown);
                    }
                    "defineclasswithbuffer" | "callruntime.definesendableclass" => {
                        let constructor = resolved(file, method.offset, i, "method_id");
                        let buffer = resolved(file, method.offset, i, "literalarray_id");
                        if let (Some(constructor), Some(buffer)) = (constructor, buffer) {
                            let name = file
                                .method_at(constructor)
                                .ok()
                                .map(|m| printer::demangle_or_offset(&m.name, m.offset))
                                .filter(|n| identifier(n));
                            let parent = match get(0) {
                                Value::Class(idx) => Some(Some(self.classes[idx].name.clone())),
                                Value::Name(name) => Some(Some(name)),
                                Value::Empty => Some(None),
                                _ => None,
                            };
                            if let (Some(name), Some(parent)) = (name, parent) {
                                let members = buffer_members(
                                    file,
                                    buffer,
                                    mn == "callruntime.definesendableclass",
                                );
                                queue.push_back((constructor, env.clone()));
                                for m in &members {
                                    queue.push_back((m.method, env.clone()));
                                }
                                let idx = self.classes.len();
                                self.classes.push(RecoveredClass {
                                    owner: method.offset,
                                    name,
                                    constructor,
                                    buffer,
                                    parent,
                                    members,
                                    initializers: Vec::new(),
                                    sendable: mn == "callruntime.definesendableclass",
                                });
                                acc = Value::Class(idx);
                                active_class = Some(idx);
                            } else {
                                acc = Value::Unknown;
                            }
                        } else {
                            acc = Value::Unknown;
                        }
                    }
                    "callruntime.createprivateproperty" => {
                        // 方法/访问器私有布局继续保留诊断；只有空方法表的数据字段可证明。
                        let empty = resolved(file, method.offset, i, "literalarray_id")
                            .and_then(|off| file.read_literal_array(off).ok())
                            .is_some_and(|values| {
                                matches!(values.as_slice(), [] | [LiteralValue::Int32(0)])
                            });
                        if empty
                            && let Some(&frame) = env.last()
                            && let Some(&count) = imms.first()
                            && count <= 4_096
                        {
                            let start = (0..)
                                .find(|slot| !self.frames[frame].values.contains_key(slot))
                                .unwrap();
                            for slot in start..start + count {
                                self.private_keys.insert(
                                    (frame, slot),
                                    format!("private_{:x}_{:x}_slot{slot}", method.offset, i.pc),
                                );
                                self.frames[frame].values.insert(slot, Value::Unknown);
                            }
                            if count > 0 {
                                self.entry_bindings
                                    .entry(method.offset)
                                    .or_default()
                                    .insert(i.pc, (frame, start));
                            }
                        }
                        acc = Value::Unknown;
                    }
                    "createobjectwithbuffer" => {
                        if let Some(buffer) = resolved(file, method.offset, i, "literalarray_id") {
                            for member in buffer_members(file, buffer, false) {
                                queue.push_back((member.method, env.clone()));
                                self.embedded_methods.insert(member.method);
                            }
                        }
                        acc = Value::Unknown;
                    }
                    "definefunc" | "definemethod" | "definefuncwithname" => {
                        acc = if let Some(off) = resolved(file, method.offset, i, "method_id") {
                            queue.push_back((off, env.clone()));
                            if mn == "definemethod"
                                && let Value::Class(idx) | Value::Prototype(idx) = acc
                                && active_class == Some(idx)
                            {
                                construction_methods.insert(off, idx);
                            }
                            if mn == "definefunc"
                                && method.offset == self.main
                                && let Ok(m) = file.method_at(off)
                                && identifier(&m.name)
                            {
                                self.functions.insert(off, m.name);
                            } else if mn == "definefunc" {
                                self.embedded_methods.insert(off);
                                let owner = self
                                    .receiver_owners
                                    .get(&method.offset)
                                    .copied()
                                    .unwrap_or(Some(method.offset));
                                self.receiver_owners
                                    .entry(off)
                                    .and_modify(|previous| {
                                        if *previous != owner {
                                            *previous = None;
                                        }
                                    })
                                    .or_insert(owner);
                            }
                            Value::Function(off)
                        } else {
                            Value::Unknown
                        };
                    }
                    "ldobjbyname" => {
                        acc = match (acc.clone(), string()) {
                            (Value::Class(idx), Some(name)) if name == "prototype" => {
                                Value::Prototype(idx)
                            }
                            (Value::Name(base), Some(name)) if identifier(&name) => {
                                Value::Name(format!("{base}.{name}"))
                            }
                            _ => Value::Unknown,
                        };
                    }
                    "stownbyname" | "stobjbyname" => {
                        if let (Value::Function(off), Some(name)) = (&acc, string()) {
                            let (idx, static_) = match get(0) {
                                Value::Class(idx) => (Some(idx), true),
                                Value::Prototype(idx) => (Some(idx), false),
                                _ => (None, false),
                            };
                            if let Some(idx) = idx
                                && active_class == Some(idx)
                                && construction_methods.get(off) == Some(&idx)
                            {
                                self.classes[idx].members.push(Member {
                                    name,
                                    method: *off,
                                    static_,
                                    accessor: None,
                                });
                            }
                        }
                    }
                    "definegettersetterbyvalue" => {
                        let (idx, static_) = match get(0) {
                            Value::Class(idx) => (Some(idx), true),
                            Value::Prototype(idx) => (Some(idx), false),
                            _ => (None, false),
                        };
                        if let (Some(idx), Value::String(name)) = (idx, get(1))
                            && active_class == Some(idx)
                        {
                            for (n, kind) in [(2, "get"), (3, "set")] {
                                if let Value::Function(off) = get(n)
                                    && construction_methods.get(&off) == Some(&idx)
                                {
                                    self.classes[idx].members.push(Member {
                                        name: name.clone(),
                                        method: off,
                                        static_,
                                        accessor: Some(kind),
                                    });
                                }
                            }
                        } else {
                            self.accessors_complete = false;
                        }
                    }
                    "callthis0" => {
                        if let (Value::Function(off), Value::Class(idx)) = (&acc, get(0))
                            && active_class == Some(idx)
                            && construction_methods.get(off) == Some(&idx)
                            && file
                                .method_at(*off)
                                .is_ok_and(|m| m.name.contains("static_initializer"))
                        {
                            self.classes[idx].initializers.push(*off);
                        }
                        acc = Value::Unknown;
                    }
                    _ if mn.starts_with("throw.") => {}
                    _ if i.meta.acc_write => acc = Value::Unknown,
                    _ => {}
                }
                if i.meta.jump
                    || (mn.starts_with("call") && mn != "callruntime.definesendableclass")
                    || matches!(mn, "poplexenv" | "stmodulevar" | "wide.stmodulevar")
                {
                    active_class = None;
                }
            }
        }
    }

    fn binding(&self, method: u32, depth: u32, slot: u32) -> Option<String> {
        let frame = self.bindings.get(&method)?.get(&(depth, slot))?.as_ref()?;
        self.frame_binding(*frame, slot)
    }

    fn frame_binding_candidate(&self, frame_id: usize, slot: u32) -> Option<(String, bool)> {
        if let Some(name) = self.private_keys.get(&(frame_id, slot)) {
            return Some((name.clone(), false));
        }
        let frame = &self.frames[frame_id];
        match frame.values.get(&slot) {
            Some(Value::Class(idx)) => Some((self.classes[*idx].name.clone(), false)),
            Some(Value::Function(off)) if self.functions.contains_key(off) => {
                self.functions.get(off).cloned().map(|name| (name, false))
            }
            _ if frame.entry => Some((
                frame
                    .names
                    .get(slot as usize)
                    .map(|name| {
                        if name == "this" {
                            "capturedThis"
                        } else {
                            name.as_str()
                        }
                    })
                    .filter(|name| identifier(name))
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("captured_{:x}_{slot}", frame.owner)),
                true,
            )),
            _ => None,
        }
    }

    fn frame_binding(&self, frame_id: usize, slot: u32) -> Option<String> {
        let (name, is_lexical_name) = self.frame_binding_candidate(frame_id, slot)?;
        if !is_lexical_name {
            return Some(name);
        }
        let frame = &self.frames[frame_id];
        // 槽位可稀疏且来自字节码立即数，只遍历实际记录的槽位，避免按最大值扩张。
        let slots: HashSet<u32> = self
            .frames
            .iter()
            .filter(|other| other.owner == frame.owner)
            .flat_map(|other| (0..other.names.len() as u32).chain(other.values.keys().copied()))
            .chain(std::iter::once(slot))
            .collect();
        let collides = self.frames.iter().enumerate().any(|(other_id, other)| {
            other.owner == frame.owner
                && slots.iter().any(|&other_slot| {
                    (other_id != frame_id || other_slot != slot)
                        && self
                            .frame_binding_candidate(other_id, other_slot)
                            .is_some_and(|(other_name, other_is_lexical_name)| {
                                other_is_lexical_name && other_name == name
                            })
                })
        });
        if !collides {
            return Some(name);
        }

        // 调试名与回退名都可能跨环境重复；仅在冲突时附加环境与槽位身份。
        let mut unique = format!("{name}_frame{frame_id}_slot{slot}");
        while self.frames.iter().enumerate().any(|(other_id, other)| {
            other.owner == frame.owner
                && slots.iter().any(|&other_slot| {
                    (other_id != frame_id || other_slot != slot)
                        && self
                            .frame_binding_candidate(other_id, other_slot)
                            .is_some_and(|(other_name, _)| other_name == unique)
                })
        }) {
            unique.push_str("_lex");
        }
        Some(unique)
    }

    pub(crate) fn lexical_names(&self, method: u32) -> lift::LexicalNames {
        let names = self
            .entry_bindings
            .get(&method)
            .into_iter()
            .flatten()
            .filter_map(|(pc, (frame, slot))| {
                self.frame_binding(*frame, *slot).map(|name| (*pc, name))
            })
            .collect();
        let owner = self
            .receiver_owners
            .get(&method)
            .copied()
            .unwrap_or(Some(method));
        let this_loads = self.entry_bindings.get(&method).into_iter().flatten().filter_map(|(pc, (frame, slot))| {
            matches!(self.frames[*frame].values.get(slot), Some(Value::Receiver(value)) if Some(*value) == owner).then_some(*pc)
        }).collect();
        lift::LexicalNames { names, this_loads }
    }

    pub(crate) fn rewrite_body(&self, method: u32, body: &mut LiftedBody) {
        let mut declarations = HashSet::new();
        declarations.extend(
            self.private_keys
                .iter()
                .filter(|((frame, _), _)| self.frames[*frame].owner == method)
                .map(|(_, name)| name.clone()),
        );
        let bindings = self
            .bindings
            .get(&method)
            .into_iter()
            .flatten()
            .filter_map(|((_, slot), frame)| frame.map(|frame| (frame, *slot)))
            .chain(
                self.entry_bindings
                    .get(&method)
                    .into_iter()
                    .flatten()
                    .map(|(_, binding)| *binding),
            );
        for (frame, slot) in bindings {
            if self.frames[frame].owner == method
                && !matches!(self.frames[frame].values.get(&slot), Some(Value::Class(_)))
                && !matches!(self.frames[frame].values.get(&slot), Some(Value::Function(off)) if self.functions.contains_key(off))
                && let Some(name) = self.frame_binding(frame, slot)
            {
                declarations.insert(name);
            }
        }
        self.insert_classes(&mut body.stmts);
        visit_stmts(&mut body.stmts, &mut |expr| match expr {
            Expr::LexVar(depth, slot) => {
                if let Some(name) = self.binding(method, *depth, *slot) {
                    *expr = Expr::Name(name);
                }
            }
            Expr::ClassLit(buffer) => {
                if let Some(class) = self.classes.iter().find(|c| c.buffer == *buffer) {
                    *expr = Expr::Name(class.name.clone());
                }
            }
            _ => {}
        });
        if method == self.main || self.classes.iter().any(|class| class.owner == method) {
            self.clean_main(&mut body.stmts, &mut HashMap::new());
        }
        let mut declarations: Vec<_> = declarations.into_iter().collect();
        declarations.sort();
        body.stmts.splice(
            0..0,
            declarations.into_iter().map(|n| Stmt::Declare(n, None)),
        );
    }

    fn insert_classes(&self, stmts: &mut Vec<Stmt>) {
        let mut out = Vec::with_capacity(stmts.len());
        for mut stmt in std::mem::take(stmts) {
            let mut buffers = Vec::new();
            if let Stmt::Expr(expr)
            | Stmt::Throw(expr)
            | Stmt::Return(Some(expr))
            | Stmt::If(expr, _, _) = &mut stmt
            {
                visit_expr(expr, &mut |e| {
                    if let Expr::ClassLit(buffer) = e
                        && !buffers.contains(buffer)
                    {
                        buffers.push(*buffer);
                    }
                });
            }
            for buffer in buffers {
                if let Some(class) = self.classes.iter().find(|c| c.buffer == buffer) {
                    out.push(Stmt::ClassDecl(class.constructor));
                }
            }
            match &mut stmt {
                Stmt::If(_, yes, no) => {
                    self.insert_classes(yes);
                    self.insert_classes(no);
                }
                Stmt::Try(body, catches) => {
                    self.insert_classes(body);
                    for (_, b) in catches {
                        self.insert_classes(b);
                    }
                }
                Stmt::TryFinally(body, cleanup) => {
                    self.insert_classes(body);
                    self.insert_classes(cleanup);
                }
                _ => {}
            }
            out.push(stmt);
        }
        *stmts = out;
    }

    fn class_value(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Name(name) => self.classes.iter().any(|c| c.name == *name),
            Expr::Member(base, prop) if prop == "prototype" => self.class_value(base),
            _ => false,
        }
    }

    /// 去除只做原样重抛的词法环境保护壳；类声明保持原始初始化顺序和模块作用域。
    pub(crate) fn finish_main(&self, file: &AbcFile, body: &mut LiftedBody) {
        fn rethrows(name: &str, stmts: &[Stmt]) -> bool {
            let mut aliases = HashSet::new();
            for (index, stmt) in stmts.iter().enumerate() {
                let same = |expr: &Expr| {
                    matches!(expr, Expr::Name(n) if n == name)
                        || matches!(expr, Expr::Reg(r) if aliases.contains(r))
                };
                match stmt {
                    Stmt::Expr(Expr::Assign(target, value)) if same(value) => {
                        if let Expr::Reg(reg) = target.as_ref() {
                            aliases.insert(*reg);
                        } else {
                            return false;
                        }
                    }
                    Stmt::Throw(value) => return index + 1 == stmts.len() && same(value),
                    _ => return false,
                }
            }
            false
        }
        fn flatten(stmts: &mut Vec<Stmt>) {
            let mut out = Vec::new();
            for mut stmt in std::mem::take(stmts) {
                match &mut stmt {
                    Stmt::Try(inner, catches) => {
                        flatten(inner);
                        for (_, b) in catches.iter_mut() {
                            flatten(b);
                        }
                        if catches.len() == 1
                            && let (Some(name), catch) = &catches[0]
                            && rethrows(name, catch)
                        {
                            out.append(inner);
                            continue;
                        }
                    }
                    Stmt::If(_, yes, no) => {
                        flatten(yes);
                        flatten(no);
                    }
                    Stmt::TryFinally(inner, cleanup) => {
                        flatten(inner);
                        flatten(cleanup);
                    }
                    _ => {}
                }
                out.push(stmt);
            }
            *stmts = out;
        }
        flatten(&mut body.stmts);
        let module = self.module(file);
        let mut declare = HashSet::new();
        visit_stmts(&mut body.stmts, &mut |expr| {
            if let Expr::Name(name) = expr
                && let Some(exported) = name.strip_prefix("export:")
            {
                let local = module
                    .as_ref()
                    .and_then(|m| m.local_exports.iter().find(|e| e.export_name == exported))
                    .map(|e| e.local_name.as_str())
                    .unwrap_or(exported);
                if identifier(local) {
                    if !self.classes.iter().any(|c| c.name == local)
                        && !self.functions.values().any(|n| n == local)
                    {
                        declare.insert(local.to_string());
                    }
                    *name = local.to_string();
                }
            }
        });
        let mut declare: Vec<_> = declare.into_iter().collect();
        declare.sort();
        body.stmts
            .splice(0..0, declare.into_iter().map(|n| Stmt::Declare(n, None)));
    }

    fn clean_main(&self, stmts: &mut Vec<Stmt>, aliases: &mut HashMap<u32, Expr>) {
        stmts.retain_mut(|stmt| {
            // 类身份仅沿直线寄存器复制传播，跨控制流不泄漏别名。
            let assigned_reg = match stmt { Stmt::Expr(Expr::Assign(target, _)) => if let Expr::Reg(r) = target.as_ref() { Some(*r) } else { None }, _ => None };
            let replace = &mut |expr: &mut Expr| {
                if let Expr::Reg(reg) = expr && let Some(value) = aliases.get(reg) { *expr = value.clone(); }
            };
            match stmt {
                Stmt::If(condition, _, _) => visit_expr(condition, replace),
                Stmt::While(_, _) | Stmt::DoWhile(_, _) | Stmt::ForOf(_, _, _) => {},
                Stmt::Try(_, _) | Stmt::TryFinally(_, _) => {},
                _ => visit_stmts(std::slice::from_mut(stmt), replace),
            }
            if let Some(reg) = assigned_reg {
                if let Stmt::Expr(Expr::Assign(target, _)) = stmt { **target = Expr::Reg(reg); }
                aliases.remove(&reg);
            }
            match stmt {
                Stmt::Expr(Expr::Assign(target, value)) => {
                    if let Expr::Reg(reg) = target.as_ref() {
                        if self.class_value(value) || matches!(value.as_ref(), Expr::Closure { method_off } if self.is_member(*method_off) || self.functions.contains_key(method_off)) { aliases.insert(*reg, value.as_ref().clone()); }
                        else { aliases.remove(reg); }
                    }
                    if let (Expr::Name(lhs), Expr::Name(rhs)) = (target.as_ref(), value.as_ref())
                        && lhs.trim_start_matches("export:") == rhs && self.classes.iter().any(|c| c.name == *rhs) { return false; }
                    if matches!(target.as_ref(), Expr::LexVar(_, _)) && self.class_value(value) { return false; }
                    if let Expr::Closure { method_off } = value.as_ref() {
                        if self.member_methods.contains(method_off) && matches!(target.as_ref(), Expr::Member(base, _) if self.class_value(base)) { return false; }
                        if let Some(name) = self.functions.get(method_off)
                            && (matches!(target.as_ref(), Expr::Name(lhs) if lhs.trim_start_matches("export:") == name) || matches!(target.as_ref(), Expr::LexVar(_, _))) { return false; }
                    }
                }
                Stmt::Expr(expr) if self.class_value(expr) => return false,
                Stmt::Expr(Expr::Closure { method_off }) if self.is_member(*method_off) || self.functions.contains_key(method_off) => return false,
                Stmt::Comment(text) if self.accessors_complete && text.starts_with("definegettersetterbyvalue ") => return false,
                Stmt::Expr(Expr::Call { callee, .. }) if matches!(callee.as_ref(), Expr::Closure { method_off } if self.classes.iter().any(|c| c.initializers.contains(method_off))) => return false,
                Stmt::If(_, yes, no) => { self.clean_main(yes, &mut aliases.clone()); self.clean_main(no, &mut aliases.clone()); aliases.clear(); }
                Stmt::Try(body, catches) => {
                    let mut after = aliases.clone();
                    self.clean_main(body, &mut after);
                    for (_, b) in catches.iter_mut() { self.clean_main(b, &mut HashMap::new()); }
                    if catches.iter().all(|(_, b)| matches!(b.last(), Some(Stmt::Throw(_) | Stmt::Return(_)))) { *aliases = after; } else { aliases.clear(); }
                }
                Stmt::TryFinally(body, cleanup) => { self.clean_main(body, &mut aliases.clone()); self.clean_main(cleanup, &mut HashMap::new()); aliases.clear(); }
                Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::ForOf(_, _, body) => { self.clean_main(body, &mut HashMap::new()); aliases.clear(); }
                _ => {}
            }
            true
        });
    }

    pub(crate) fn render_class(
        &self,
        file: &AbcFile,
        constructor: u32,
        cache: &Rc<RefCell<printer::NestedCache>>,
    ) -> Option<String> {
        let record = file.classes.iter().find(|c| c.offset == self.record)?;
        let module = super::module_record_of(file, record);
        let class = self.classes.iter().find(|c| c.constructor == constructor)?;
        let extends = class
            .parent
            .as_ref()
            .map(|n| format!(" extends {n}"))
            .unwrap_or_default();
        let mut out = format!("class {}{extends} {{\n", class.name);
        if class.sendable {
            out.insert_str(0, "@Sendable\n");
        }
        let mut members = vec![Member {
            name: "constructor".into(),
            method: class.constructor,
            static_: false,
            accessor: None,
        }];
        members.extend(class.members.clone());
        for member in members {
            if let Some(mut text) =
                self.render_member(file, record, module.as_ref(), cache, &member)
            {
                if let Some(accessor) = member.accessor {
                    text = format!("{accessor} {text}");
                }
                if member.static_ {
                    text = format!("static {text}");
                }
                for line in text.lines() {
                    out.push_str("    ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        for off in &class.initializers {
            if let Some(fields) = self.constant_fields(file, record, module.as_ref(), *off) {
                out.push_str(&fields);
                continue;
            }
            let member = Member {
                name: "__initialize".into(),
                method: *off,
                static_: true,
                accessor: None,
            };
            if let Some(text) = self.render_member(file, record, module.as_ref(), cache, &member) {
                out.push_str("    static {\n");
                let lines: Vec<_> = text.lines().collect();
                for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
                    if line.trim() != "return;" {
                        out.push_str("    ");
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                out.push_str("    }\n");
            }
        }
        out.push_str("}\n");
        Some(out)
    }

    /// 只把完整、纯字面量的静态初始化器折叠成字段；任意其它操作保留 static 块。
    fn constant_fields(
        &self,
        file: &AbcFile,
        _record: &ClassInfo,
        module: Option<&ModuleRecord>,
        offset: u32,
    ) -> Option<String> {
        let method = file.method_at(offset).ok()?;
        let code = method.code.as_ref()?;
        if code.code_size > 20_000 {
            return None;
        }
        let ins = crate::disasm::decode(code.instructions(file.bytes())).ok()?;
        if ins.len() > 4_000 {
            return None;
        }
        let graph = cfg::build(&ins, code.code_size as usize, &code.try_blocks);
        let mut body = lift::lift_with_lexical_names(
            file,
            &method,
            code,
            &graph,
            module,
            self.lexical_names(method.offset),
        );
        self.rewrite_body(method.offset, &mut body);
        structure::polish(&mut body.stmts);
        constant_field_declarations(&body.stmts)
    }

    fn render_member(
        &self,
        file: &AbcFile,
        _record: &ClassInfo,
        module: Option<&ModuleRecord>,
        cache: &Rc<RefCell<printer::NestedCache>>,
        member: &Member,
    ) -> Option<String> {
        let mut method = file.method_at(member.method).ok()?;
        let Some(code) = method.code.as_ref() else {
            let params = match member.accessor {
                Some("get") => "",
                Some("set") => "value",
                _ => "...args",
            };
            return Some(format!(
                "{}({params}) {{\n    // No bytecode body for method @{:#x}; see disassembly.\n    throw new Error(\"Unresolved method @{:#x}\");\n}}",
                member.name, member.method, member.method
            ));
        };
        let ins = crate::disasm::decode_with(
            code.instructions(file.bytes()),
            file.header.kind == crate::file::HeaderKind::Legacy,
        );
        let fallback = || {
            let body = LiftedBody {
                is_async: false,
                is_concurrent: false,
                stmts: vec![
                    Stmt::Comment(format!(
                        "method @{:#x}: body unavailable or exceeds structural analysis limit; see disassembly",
                        method.offset
                    )),
                    Stmt::Throw(Expr::Call {
                        callee: Box::new(Expr::Name("Error".into())),
                        args: vec![Expr::Str(format!(
                            "Unresolved method @{:#x}",
                            method.offset
                        ))],
                        new: true,
                        this_arg: None,
                    }),
                ],
            };
            let mut named = method.clone();
            named.name = member.name.clone();
            printer::render_nested(file, &named, code, &body, 0, NestedStyle::Method, cache)
        };
        let Ok(ins) = ins else {
            return Some(fallback());
        };
        if code.code_size > 20_000 || ins.len() > 4_000 {
            return Some(fallback());
        }
        let graph = cfg::build(&ins, code.code_size as usize, &code.try_blocks);
        let mut body = lift::lift_with_lexical_names(
            file,
            &method,
            code,
            &graph,
            module,
            self.lexical_names(method.offset),
        );
        self.rewrite_body(method.offset, &mut body);
        if member.name == "constructor" {
            constructor_body(&mut body.stmts);
        }
        structure::polish(&mut body.stmts);
        if member.name == "constructor" {
            constructor_body(&mut body.stmts);
            structure::polish(&mut body.stmts);
        }
        if member.name == "constructor"
            && matches!(
                body.stmts.last(),
                Some(Stmt::Return(
                    None | Some(Expr::Arg(2) | Expr::Special("this" | "undefined"))
                ))
            )
        {
            body.stmts.pop();
        }
        method.name = member.name.clone();
        Some(printer::render_nested(
            file,
            &method,
            code,
            &body,
            0,
            NestedStyle::Method,
            cache,
        ))
    }
}

fn constant_field_declarations(stmts: &[Stmt]) -> Option<String> {
    let mut out = String::new();
    let mut receiver_copies = HashSet::new();
    for (index, stmt) in stmts.iter().enumerate() {
        let is_receiver = |expr: &Expr| {
            matches!(expr, Expr::Arg(2) | Expr::Special("this"))
                || matches!(expr, Expr::Reg(reg) if receiver_copies.contains(reg))
        };
        if let Stmt::Expr(Expr::Assign(target, value)) = stmt
            && let Expr::Reg(reg) = target.as_ref()
            && is_receiver(value)
        {
            receiver_copies.insert(*reg);
            continue;
        }
        if index + 1 == stmts.len()
            && matches!(stmt, Stmt::Return(None | Some(Expr::Special("undefined"))))
        {
            continue;
        }
        let Stmt::Expr(Expr::Call {
            callee,
            args,
            new: false,
            this_arg: None,
        }) = stmt
        else {
            return None;
        };
        if !matches!(
            callee.as_ref(),
            Expr::Special("globalThis.Object.defineProperty")
        ) {
            return None;
        }
        let [receiver, Expr::Str(key), Expr::Object(fields)] = args.as_slice() else {
            return None;
        };
        if !is_receiver(receiver) {
            return None;
        }
        let [
            (value_key, value),
            (writable, Expr::Bool(true)),
            (enumerable, Expr::Bool(true)),
            (configurable, Expr::Bool(true)),
        ] = fields.as_slice()
        else {
            return None;
        };
        if value_key != "value"
            || writable != "writable"
            || enumerable != "enumerable"
            || configurable != "configurable"
        {
            return None;
        }
        let value = match value {
            Expr::Str(s) => format!("{s:?}"),
            Expr::Int(n) => n.to_string(),
            Expr::Float(n) if n.is_finite() => n.to_string(),
            Expr::Bool(b) => b.to_string(),
            Expr::Special("null") => "null".into(),
            // 不把闭包变为字段初始化式，避免引入原指令没有的函数 name 推断。
            _ => return None,
        };
        let key = if identifier(key) && !matches!(key.as_str(), "constructor" | "prototype") {
            key.clone()
        } else {
            format!("[{key:?}]")
        };
        out.push_str(&format!("    static {key} = {value};\n"));
    }
    (!out.is_empty()).then_some(out)
}

fn buffer_members(file: &AbcFile, buffer: u32, sendable: bool) -> Vec<Member> {
    let Ok(mut values) = file.read_literal_array(buffer) else {
        return Vec::new();
    };
    // Sendable 缓冲区末尾额外携带字段类型表索引；其前一项才是实例成员数。
    if sendable {
        values.pop();
    }
    let instance_count = match values.last() {
        Some(LiteralValue::Int32(n)) if *n >= 0 => *n as usize,
        _ => usize::MAX,
    };
    let mut members = Vec::new();
    let mut name = None;
    for value in &values {
        match value {
            LiteralValue::String(off) => name = file.string_at(*off).ok(),
            LiteralValue::Method(off)
            | LiteralValue::GeneratorMethod(off)
            | LiteralValue::AsyncGeneratorMethod(off)
            | LiteralValue::Getter(off)
            | LiteralValue::Setter(off) => {
                if let Some(name) = name.take() {
                    let accessor = match value {
                        LiteralValue::Getter(_) => Some("get"),
                        LiteralValue::Setter(_) => Some("set"),
                        _ => None,
                    };
                    members.push(Member {
                        name,
                        method: *off,
                        static_: members.len() >= instance_count,
                        accessor,
                    });
                }
            }
            _ => {}
        }
    }
    members
}

fn constructor_body(stmts: &mut Vec<Stmt>) {
    // 默认参数分支可以位于 VM 接收者复制与 super 之间；只跨过不访问
    // 该接收者、没有 super 且不会提前结束控制流的初始化语句。
    fn independent_initialization(stmt: &Stmt, receiver: u32) -> bool {
        let straight = match stmt {
            Stmt::Expr(_) => true,
            Stmt::If(_, yes, no) => yes
                .iter()
                .chain(no)
                .all(|stmt| independent_initialization(stmt, receiver)),
            _ => false,
        };
        if !straight {
            return false;
        }
        let mut dependent = false;
        visit_stmts(std::slice::from_mut(&mut stmt.clone()), &mut |expr| {
            dependent |= matches!(expr, Expr::Reg(reg) if *reg == receiver)
                || matches!(expr, Expr::Assign(target, _) | Expr::Update(_, target, _)
                    if matches!(target.as_ref(), Expr::Reg(reg) if *reg == receiver))
                || matches!(expr, Expr::Call { callee, .. }
                    if matches!(callee.as_ref(), Expr::Special("super")));
        });
        !dependent
    }
    // 参数展开数组刚建立、尚未逃逸时，编译器留下的无用 length 读取可消除。
    let mut index = 1;
    while index < stmts.len() {
        if matches!((&stmts[index - 1], &stmts[index]),
            (Stmt::Expr(Expr::Assign(target, value)), Stmt::Expr(Expr::Member(base, name)))
                if matches!(target.as_ref(), Expr::Reg(_))
                    && matches!(value.as_ref(), Expr::Array(_)) && name == "length"
                    && printer::expr_eq_bounded(target, base, &mut 64))
        {
            stmts.remove(index);
        } else {
            index += 1;
        }
    }
    // 派生构造器的 VM 接收者起初是 hole。调试 prologue 可把它复制到
    // 随后被 super 返回值覆盖的槽；没有中间读取时去掉该机器复制。
    let mut copies = HashSet::new();
    for (index, stmt) in stmts.iter().enumerate() {
        let Stmt::Expr(Expr::Assign(target, value)) = stmt else {
            continue;
        };
        let Expr::Reg(reg) = target.as_ref() else {
            continue;
        };
        if !matches!(value.as_ref(), Expr::Arg(2) | Expr::Special("this")) {
            continue;
        }
        let mut super_results = HashSet::new();
        for next in &stmts[index + 1..] {
            // 只追踪直线赋值；分支里的 super 不保证覆盖原接收者副本。
            let Stmt::Expr(Expr::Assign(target, value)) = next else {
                if independent_initialization(next, *reg) {
                    super_results.clear();
                    continue;
                }
                break;
            };
            let mut reads_receiver = false;
            let mut next_clone = next.clone();
            visit_stmts(std::slice::from_mut(&mut next_clone), &mut |expr| {
                if matches!(expr, Expr::Reg(r) if r == reg) {
                    reads_receiver = true;
                }
            });
            if reads_receiver {
                break;
            }
            let Expr::Reg(target) = target.as_ref() else {
                break;
            };
            let from_super = matches!(value.as_ref(), Expr::Call { callee, .. }
                if matches!(callee.as_ref(), Expr::Special("super")))
                || matches!(value.as_ref(), Expr::Reg(r) if super_results.contains(r));
            if target == reg {
                if from_super {
                    copies.insert(index);
                }
                break;
            }
            if from_super {
                super_results.insert(*target);
            } else {
                super_results.remove(target);
            }
        }
    }
    let mut index = 0;
    stmts.retain(|_| {
        let keep = !copies.contains(&index);
        index += 1;
        keep
    });
    // 寄存器复用跨分支时不能传播 this；只消除整方法唯一写入的接收者副本。
    let mut writes = HashMap::<u32, usize>::new();
    visit_stmts(stmts, &mut |expr| {
        if let Expr::Assign(target, _) | Expr::Update(_, target, _) = expr
            && let Expr::Reg(reg) = target.as_ref()
        {
            *writes.entry(*reg).or_default() += 1;
        }
    });
    let mut receivers = HashSet::new();
    stmts.retain_mut(|stmt| {
        let assigned = match stmt {
            Stmt::Expr(Expr::Assign(target, _)) => {
                if let Expr::Reg(r) = target.as_ref() {
                    Some(*r)
                } else {
                    None
                }
            }
            _ => None,
        };
        visit_stmts(std::slice::from_mut(stmt), &mut |expr| {
            if matches!(expr, Expr::Reg(r) if receivers.contains(r)) {
                *expr = Expr::Special("this");
            }
        });
        if let Some(reg) = assigned {
            receivers.remove(&reg);
        }
        if let Stmt::Expr(Expr::Assign(target, value)) = stmt
            && let Expr::Reg(reg) = target.as_ref()
            && writes.get(reg) == Some(&1)
            && matches!(value.as_ref(), Expr::Arg(2) | Expr::Special("this"))
        {
            receivers.insert(*reg);
            return false;
        }
        if let Stmt::Expr(Expr::Assign(target, value)) = stmt
            && let (Expr::Reg(reg), Expr::Call { callee, .. }) = (target.as_ref(), value.as_ref())
            && matches!(callee.as_ref(), Expr::Special("super"))
            && writes.get(reg) == Some(&1)
        {
            receivers.insert(*reg);
            *stmt = Stmt::Expr(value.as_ref().clone());
        }
        true
    });
    if matches!(
        stmts.last(),
        Some(Stmt::Return(Some(Expr::Arg(2) | Expr::Special("this"))))
    ) {
        stmts.pop();
    }
}

fn visit_expr(expr: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
    match expr {
        Expr::Object(fields) => {
            for (_, value) in fields {
                visit_expr(value, f);
            }
        }
        Expr::Array(elements) => {
            for value in elements {
                visit_expr(value, f);
            }
        }
        Expr::Bin(_, a, b) | Expr::Index(a, b) => {
            visit_expr(a, f);
            visit_expr(b, f);
        }
        Expr::Assign(a, b) => {
            if !matches!(a.as_ref(), Expr::Reg(_)) {
                visit_expr(a, f);
            }
            visit_expr(b, f);
        }
        Expr::Un(_, e)
        | Expr::Await(e)
        | Expr::Update(_, e, _)
        | Expr::Member(e, _)
        | Expr::Spread(e) => visit_expr(e, f),
        Expr::Cond(c, a, b) => {
            visit_expr(c, f);
            visit_expr(a, f);
            visit_expr(b, f);
        }
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            visit_expr(callee, f);
            for arg in args {
                visit_expr(arg, f);
            }
            if let Some(arg) = this_arg {
                visit_expr(arg, f);
            }
        }
        _ => {}
    }
    f(expr);
}

pub(super) fn visit_stmts(stmts: &mut [Stmt], f: &mut impl FnMut(&mut Expr)) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) | Stmt::Declare(_, Some(e)) => {
                visit_expr(e, f)
            }
            Stmt::If(e, yes, no) => {
                visit_expr(e, f);
                visit_stmts(yes, f);
                visit_stmts(no, f);
            }
            Stmt::While(e, body) | Stmt::ForOf(_, e, body) | Stmt::DoWhile(body, e) => {
                visit_expr(e, f);
                visit_stmts(body, f);
            }
            Stmt::Try(body, catches) => {
                visit_stmts(body, f);
                for (_, body) in catches {
                    visit_stmts(body, f);
                }
            }
            Stmt::TryFinally(body, cleanup) => {
                visit_stmts(body, f);
                visit_stmts(cleanup, f);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assign(reg: u32, value: Expr) -> Stmt {
        Stmt::Expr(Expr::Assign(Box::new(Expr::Reg(reg)), Box::new(value)))
    }

    #[test]
    fn constructor_array_cleanup_retains_property_getters() {
        let target = Expr::Member(Box::new(Expr::Special("this")), "items".into());
        let mut stmts = vec![
            Stmt::Expr(Expr::Assign(
                Box::new(target.clone()),
                Box::new(Expr::Array(Vec::new())),
            )),
            Stmt::Expr(Expr::Member(Box::new(target), "length".into())),
        ];
        constructor_body(&mut stmts);
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[1], Stmt::Expr(Expr::Member(base, name))
            if matches!(base.as_ref(), Expr::Member(_, _)) && name == "length"));
    }

    fn use_reg(reg: u32) -> Stmt {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::Name("observe".into())),
            args: vec![Expr::Reg(reg)],
            new: false,
            this_arg: None,
        })
    }

    fn recovery() -> RecordRecovery {
        RecordRecovery {
            classes: vec![RecoveredClass {
                owner: 0,
                sendable: false,
                name: "C".into(),
                constructor: 1,
                buffer: 2,
                parent: None,
                members: vec![],
                initializers: vec![],
            }],
            ..RecordRecovery::default()
        }
    }

    #[test]
    fn class_aliases_do_not_escape_a_loop_that_changes_the_register() {
        let mut stmts = vec![
            assign(0, Expr::Name("C".into())),
            Stmt::While(
                Expr::Bool(true),
                vec![assign(0, Expr::Name("Other".into()))],
            ),
            use_reg(0),
        ];
        recovery().clean_main(&mut stmts, &mut HashMap::new());
        assert!(
            matches!(stmts.last(), Some(Stmt::Expr(Expr::Call { args, .. })) if matches!(args.as_slice(), [Expr::Reg(0)]))
        );
    }

    #[test]
    fn receiver_aliases_are_not_substituted_after_a_conditional_write() {
        let mut stmts = vec![
            assign(0, Expr::Arg(2)),
            Stmt::If(
                Expr::Bool(true),
                vec![assign(0, Expr::ObjectLit(0)), use_reg(0)],
                vec![],
            ),
        ];
        constructor_body(&mut stmts);
        assert!(
            matches!(&stmts[1], Stmt::If(_, yes, _) if matches!(yes.last(), Some(Stmt::Expr(Expr::Call { args, .. })) if matches!(args.as_slice(), [Expr::Reg(0)])))
        );
    }

    #[test]
    fn dynamic_prototype_closures_stay_at_the_assignment_site() {
        let mut recovery = recovery();
        recovery.embedded_methods.insert(7);
        let target = Expr::Member(
            Box::new(Expr::Member(
                Box::new(Expr::Name("C".into())),
                "prototype".into(),
            )),
            "extra".into(),
        );
        let mut stmts = vec![Stmt::If(
            Expr::Name("flag".into()),
            vec![Stmt::Expr(Expr::Assign(
                Box::new(target),
                Box::new(Expr::Closure { method_off: 7 }),
            ))],
            vec![],
        )];
        recovery.clean_main(&mut stmts, &mut HashMap::new());
        assert!(
            matches!(&stmts[0], Stmt::If(_, yes, _) if matches!(yes.as_slice(), [Stmt::Expr(Expr::Assign(_, _))]))
        );
    }

    #[test]
    fn derived_receiver_is_not_read_before_super() {
        let super_call = Expr::Call {
            callee: Box::new(Expr::Special("super")),
            args: vec![],
            new: false,
            this_arg: None,
        };
        let mut stmts = vec![assign(2, Expr::Arg(2)), assign(2, super_call), use_reg(2)];
        constructor_body(&mut stmts);
        assert!(
            matches!(&stmts[0], Stmt::Expr(Expr::Call { callee, .. }) if matches!(callee.as_ref(), Expr::Special("super")))
        );
        assert!(
            matches!(&stmts[1], Stmt::Expr(Expr::Call { args, .. }) if matches!(args.as_slice(), [Expr::Special("this")]))
        );
    }

    #[test]
    fn derived_receiver_cleanup_requires_an_unconditional_unclobbered_super_result() {
        let super_call = Expr::Call {
            callee: Box::new(Expr::Special("super")),
            args: vec![],
            new: false,
            this_arg: None,
        };
        let cases = [
            vec![
                assign(2, Expr::Arg(2)),
                Stmt::If(Expr::Arg(3), vec![assign(2, super_call.clone())], vec![]),
                use_reg(2),
            ],
            vec![
                assign(2, Expr::Arg(2)),
                assign(3, super_call),
                assign(3, Expr::Int(7)),
                assign(2, Expr::Reg(3)),
                use_reg(2),
            ],
        ];
        for mut statements in cases {
            constructor_body(&mut statements);
            assert!(
                matches!(statements.first(), Some(Stmt::Expr(Expr::Assign(target, value)))
                if matches!(target.as_ref(), Expr::Reg(2))
                    && matches!(value.as_ref(), Expr::Arg(2))),
                "{statements:#?}"
            );
        }
    }

    #[test]
    fn derived_this_placeholder_is_removed_when_super_overwrites_it() {
        let super_call = Expr::Call {
            callee: Box::new(Expr::Special("super")),
            args: vec![Expr::Spread(Box::new(Expr::Arg(0)))],
            new: false,
            this_arg: None,
        };
        let mut stmts = vec![
            assign(2, Expr::Special("this")),
            assign(2, super_call),
            Stmt::Expr(Expr::Assign(
                Box::new(Expr::Member(Box::new(Expr::Reg(2)), "ready".into())),
                Box::new(Expr::Bool(true)),
            )),
        ];

        constructor_body(&mut stmts);

        assert!(
            matches!(stmts.first(), Some(Stmt::Expr(Expr::Call { callee, .. }))
            if matches!(callee.as_ref(), Expr::Special("super")))
        );
        assert!(
            matches!(stmts.get(1), Some(Stmt::Expr(Expr::Assign(target, _)))
            if matches!(target.as_ref(), Expr::Member(base, name)
                if matches!(base.as_ref(), Expr::Special("this")) && name == "ready"))
        );
    }

    #[test]
    fn derived_receiver_placeholder_is_removed_when_super_uses_a_temporary() {
        let super_call = Expr::Call {
            callee: Box::new(Expr::Special("super")),
            args: vec![Expr::Spread(Box::new(Expr::Arg(0)))],
            new: false,
            this_arg: None,
        };
        let mut stmts = vec![
            assign(2, Expr::Arg(2)),
            assign(5, super_call),
            assign(2, Expr::Reg(5)),
            assign(5, Expr::Reg(2)),
        ];

        constructor_body(&mut stmts);

        assert!(
            !matches!(stmts.first(), Some(Stmt::Expr(Expr::Assign(target, value)))
            if matches!(target.as_ref(), Expr::Reg(2)) && matches!(value.as_ref(), Expr::Arg(2)))
        );
        assert!(stmts.iter().any(|stmt| matches!(stmt, Stmt::Expr(Expr::Assign(_, value))
            if matches!(value.as_ref(), Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::Special("super"))))));
    }

    #[test]
    fn unused_class_prototype_read_still_emits_the_class_declaration() {
        let mut stmts = vec![Stmt::Expr(Expr::Member(
            Box::new(Expr::ClassLit(2)),
            "prototype".into(),
        ))];
        recovery().insert_classes(&mut stmts);
        assert!(matches!(stmts.first(), Some(Stmt::ClassDecl(1))));
    }

    #[test]
    fn imported_bases_use_bindings_and_request_indices() {
        let mut module = ModuleRecord::default();
        module.regular_imports.push(crate::file::RegularImport {
            local_name: "PafSheetConfig".into(),
            import_name: "PafSheetConfig".into(),
            module_idx: 2,
        });
        module.namespace_imports.push(crate::file::NamespaceImport {
            local_name: "SheetApi".into(),
            module_idx: 7,
        });

        assert_eq!(
            external_import_name(Some(&module), 0).as_deref(),
            Some("PafSheetConfig")
        );
        assert_eq!(
            namespace_import_name(Some(&module), 7).as_deref(),
            Some("SheetApi")
        );
        // Namespace table position zero is not request index zero.
        assert_eq!(namespace_import_name(Some(&module), 0), None);
    }

    #[test]
    fn ambiguous_namespace_request_is_not_guessed_as_a_base() {
        let mut module = ModuleRecord::default();
        module.namespace_imports.extend([
            crate::file::NamespaceImport {
                local_name: "First".into(),
                module_idx: 4,
            },
            crate::file::NamespaceImport {
                local_name: "Second".into(),
                module_idx: 4,
            },
        ]);

        assert_eq!(namespace_import_name(Some(&module), 4), None);
    }

    #[test]
    fn fallback_lexical_names_distinguish_frames_with_the_same_owner_and_slot() {
        let mut recovery = RecordRecovery::default();
        recovery.frames.extend([
            Frame {
                owner: 0x1234,
                entry: true,
                ..Frame::default()
            },
            Frame {
                owner: 0x1234,
                entry: true,
                ..Frame::default()
            },
        ]);

        let first = recovery.frame_binding(0, 0).unwrap();
        let second = recovery.frame_binding(1, 0).unwrap();
        assert_ne!(first, second);
        assert_eq!(first, "captured_1234_0_frame0_slot0");
        assert_eq!(second, "captured_1234_0_frame1_slot0");
    }

    #[test]
    fn repeated_debug_names_across_slots_are_disambiguated() {
        let mut recovery = RecordRecovery::default();
        recovery.frames.push(Frame {
            owner: 0x1234,
            names: vec!["value".into(), "value".into()],
            entry: true,
            ..Frame::default()
        });

        let first = recovery.frame_binding(0, 0).unwrap();
        let second = recovery.frame_binding(0, 1).unwrap();
        assert_ne!(first, second);
        assert_eq!(first, "value_frame0_slot0");
        assert_eq!(second, "value_frame0_slot1");
    }

    fn lexical_envs(bytes: &[u8]) -> (Vec<Ins>, HashMap<usize, Vec<usize>>) {
        let instructions = crate::disasm::decode(bytes).expect("valid lexical fixture");
        let graph = cfg::build(&instructions, bytes.len(), &[]);
        let frames = instructions
            .iter()
            .enumerate()
            .filter_map(|(id, instruction)| {
                (instruction.meta.mnemonic == "newlexenv").then_some((instruction.pc, id))
            })
            .collect();
        let environments = lexical_environments(&instructions, &graph, &[], &frames);
        (instructions, environments)
    }

    #[test]
    fn lexical_environment_survives_a_branch_join_with_the_same_stack() {
        // newlexenv 1; jeqz L_join; nop; L_join: ldlexvar 0,0; returnundefined
        let (instructions, environments) = lexical_envs(&[0x09, 1, 0x4f, 3, 0xd5, 0x3c, 0, 0x65]);
        let load = instructions
            .iter()
            .find(|instruction| instruction.meta.mnemonic == "ldlexvar")
            .unwrap();
        assert_eq!(environments.get(&load.pc), Some(&vec![0]));
    }

    #[test]
    fn lexical_environment_rejects_a_join_with_different_frame_stacks() {
        // One branch reaches L_join with [frame0], the other with [frame0, frame1].
        let (instructions, environments) =
            lexical_envs(&[0x09, 1, 0x4f, 4, 0x09, 1, 0x3c, 0, 0x65]);
        let load = instructions
            .iter()
            .find(|instruction| instruction.meta.mnemonic == "ldlexvar")
            .unwrap();
        assert!(!environments.contains_key(&load.pc));
    }

    #[test]
    fn straight_line_lexical_frames_keep_same_slot_distinct() {
        let (instructions, environments) =
            lexical_envs(&[0x09, 1, 0x09, 1, 0x3c, 0, 0x3c, 0x01, 0x65]);
        let loads: Vec<_> = instructions
            .iter()
            .filter(|instruction| instruction.meta.mnemonic == "ldlexvar")
            .collect();
        let env0 = environments.get(&loads[0].pc).unwrap();
        let env1 = environments.get(&loads[1].pc).unwrap();
        assert_eq!(env0.len(), 2);
        assert_eq!(env1.len(), 2);
        assert_ne!(env0[1], env1[0]);
        assert_eq!(env0[1], 1);
        assert_eq!(env1[0], 0);
    }

    #[test]
    fn catch_lexical_bindings_require_identical_stacks_at_every_protected_instruction() {
        use crate::file::{CatchBlock, TryBlock};
        for (bytes, start, length, handler, known) in [
            // 建立环境后抛出；catch 应继承稳定的模块环境。
            (vec![0x09, 1, 0x00, 0xfe, 0, 0x3c, 0, 0x65], 2, 3, 5, true),
            // 同一保护区内建立第二个环境，catch 入口可能携带不同栈，必须拒绝猜测。
            (
                vec![0x09, 1, 0xd5, 0x09, 1, 0x00, 0xfe, 0, 0x3c, 0, 0x65],
                2,
                6,
                8,
                false,
            ),
        ] {
            let ins = crate::disasm::decode(&bytes).unwrap();
            let regions = [TryBlock {
                start_pc: start,
                length,
                catch_blocks: vec![CatchBlock {
                    type_idx: 0,
                    handler_pc: handler,
                    code_size: 0,
                }],
            }];
            let graph = cfg::build(&ins, bytes.len(), &regions);
            let frames = ins
                .iter()
                .enumerate()
                .filter_map(|(id, instruction)| {
                    (instruction.meta.mnemonic == "newlexenv").then_some((instruction.pc, id))
                })
                .collect();
            let environments = lexical_environments(&ins, &graph, &[], &frames);
            assert_eq!(
                environments.contains_key(&(handler as usize)),
                known,
                "{environments:?}"
            );
        }
    }
}
