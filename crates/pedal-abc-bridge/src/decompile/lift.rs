//! 表达式提升：累加器/寄存器 → 表达式树，指令 → 语句
//!
//! 寄存器保留为显式变量，累加器在基本块内构建表达式；
//! 分支通过后支配者汇合，循环条件保留在循环内部，合流值显式落地。
//! 未覆盖的指令以注释形式保留，保证语义可见。
//!
//! @author ljlVink

use crate::decompile::cfg::Cfg;
use crate::disasm::Ins;
use crate::file::{AbcFile, ClassInfo, CodeInfo, MethodInfo, ModuleRecord};

/// 表达式
#[derive(Debug, Clone)]
pub enum Expr {
    /// 寄存器引用（未追踪到定义时）
    Reg(u32),
    /// 参数引用
    Arg(u32),
    /// 整数
    Int(i64),
    /// 浮点
    Float(f64),
    /// 字符串
    Str(String),
    /// 布尔
    Bool(bool),
    /// null / undefined / NaN / Infinity / this / new.target / hole
    Special(&'static str),
    /// 标识符（模块变量等已解析名字）
    Name(String),
    /// 二元/逻辑操作
    Bin(&'static str, Box<Expr>, Box<Expr>),
    /// 一元操作
    Un(&'static str, Box<Expr>),
    /// 已验证挂起/恢复协议的异步等待
    Await(Box<Expr>),
    /// 更新（++/--）
    Update(&'static str, Box<Expr>, bool),
    /// 赋值表达式
    Assign(Box<Expr>, Box<Expr>),
    /// 条件表达式
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// 成员访问
    Member(Box<Expr>, String),
    /// 索引访问
    Index(Box<Expr>, Box<Expr>),
    /// 调用；new=true 表示 new 调用；this_arg 存在时显式接收者
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        new: bool,
        this_arg: Option<Box<Expr>>,
    },
    /// 函数闭包（definefunc）
    Closure {
        /// 方法偏移
        method_off: u32,
    },
    /// 词法环境变量（depth, slot）
    LexVar(u32, u32),
    /// 数组字面量（literalarray 偏移）
    ArrayLit(u32),
    /// 对象字面量（literalarray 偏移）
    ObjectLit(u32),
    /// 已解析的对象字面量（键与值保持顺序）。
    Object(Vec<(String, Expr)>),
    /// 已解析的数组字面量（元素顺序保持）。
    Array(Vec<Expr>),
    /// 类字面量（defineclasswithbuffer 的 literal buffer 偏移）
    ClassLit(u32),
    /// 可选链调用标记暂不区分
    TemplateLit(String),
    /// 展开标记（...x，调用/数组字面量内）
    Spread(Box<Expr>),
}

/// 语句
#[derive(Debug, Clone)]
pub enum Stmt {
    /// 表达式语句
    Expr(Expr),
    /// 局部声明
    Declare(String, Option<Expr>),
    /// 已解析的类声明，保存构造方法偏移并保留模块求值顺序
    ClassDecl(u32),
    /// return
    Return(Option<Expr>),
    /// throw
    Throw(Expr),
    /// if / else
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    /// while
    While(Expr, Vec<Stmt>),
    /// do-while
    DoWhile(Vec<Stmt>, Expr),
    /// for-of（近似）
    ForOf(String, Expr, Vec<Stmt>),
    /// try/catch
    Try(Vec<Stmt>, Vec<(Option<String>, Vec<Stmt>)>),
    /// try/finally；带 catch 时在 body 中嵌套 Try
    TryFinally(Vec<Stmt>, Vec<Stmt>),
    /// break/continue
    Break,
    Continue,
    /// 未覆盖指令注释
    Comment(String),
}

/// 提升结果
pub struct LiftedBody {
    /// 语句序列
    pub stmts: Vec<Stmt>,
    /// 只有完整验证异步状态机后才输出 async
    pub is_async: bool,
    /// 完整 taskpool 返回协议确认的并发函数，渲染时恢复编译器指令序言。
    pub is_concurrent: bool,
}

/// record 分析确认的词法名称，以及与打印作用域的 this 同源的加载指令。
#[derive(Default)]
pub(crate) struct LexicalNames {
    pub(crate) names: std::collections::HashMap<usize, String>,
    pub(crate) this_loads: std::collections::HashSet<usize>,
}

struct Lifter<'a> {
    file: &'a AbcFile,
    method: &'a MethodInfo,
    module: Option<&'a ModuleRecord>,
    ins: &'a [Ins],
    cfg: &'a Cfg,
    acc: Expr,
    /// 当前 acc 值是否已落到语句/寄存器（避免 set_acc 重复补发副作用）
    acc_persisted: bool,
    /// 词法环境名表栈（newlexenvwithname 提供；None = 未命名）
    lex_names: Vec<Option<Vec<String>>>,
    /// 入口直线区间按指令 PC 确认的词法绑定，区分同一 depth/slot 的不同环境。
    lexical_names: std::collections::HashMap<usize, String>,
    regs: Vec<Option<Expr>>,
    stmts: Vec<Stmt>,
    num_vregs: u32,
    num_args: u32,
    /// 汇合值及保存累加器所用的临时寄存器
    next_temp: u32,
    active_loops: Vec<(usize, Vec<usize>)>,
    exception_regions: Vec<crate::file::TryBlock>,
    active_regions: Vec<usize>,
    is_async: bool,
    finally_candidates: Vec<(String, Vec<Stmt>, u32, bool)>,
    raw_fallback: Option<&'static str>,
    /// VM hole 是内部唯一值，不能与用户抛出的 undefined 混淆。
    hole_sentinel: Option<u32>,
    /// 接收者沿正常/异常边仍为 this 的 super 属性指令。
    super_receivers: std::collections::HashSet<usize>,
}

/// getmodulenamespace uses a module-request index, so resolve it through module_idx.
fn namespace_name_for_request(module: Option<&ModuleRecord>, request: u64) -> Option<&str> {
    let module = module?;
    let mut imports = module
        .namespace_imports
        .iter()
        .filter(|import| u64::from(import.module_idx) == request);
    let name = imports.next()?.local_name.as_str();
    imports.next().is_none().then_some(name)
}

/// Resolve an lde external-variable index to its import binding.
fn external_import_name(module: Option<&ModuleRecord>, index: usize) -> Option<&str> {
    let module = module?;
    module
        .regular_imports
        .get(index)
        .map(|import| import.local_name.as_str())
        .or_else(|| {
            module
                .namespace_imports
                .get(index)
                .map(|import| import.local_name.as_str())
        })
}

/// 提升方法体
pub fn lift(
    file: &AbcFile,
    _class: &ClassInfo,
    method: &MethodInfo,
    code: &CodeInfo,
    cfg: &Cfg,
    module: Option<&ModuleRecord>,
) -> LiftedBody {
    lift_with_lexical_names(file, method, code, cfg, module, Default::default())
}

/// 使用 record 分析确认的词法绑定提升，保留旧入口供独立方法分析使用。
pub(crate) fn lift_with_lexical_names(
    file: &AbcFile,
    method: &MethodInfo,
    code: &CodeInfo,
    cfg: &Cfg,
    module: Option<&ModuleRecord>,
    lexical_names: LexicalNames,
) -> LiftedBody {
    let bytes = code.instructions(&file.data);
    let legacy = file.header.kind == crate::file::HeaderKind::Legacy;
    let mut ins = crate::disasm::decode_with(bytes, legacy).unwrap_or_default();
    let normalized = normalize_async(&ins, &code.try_blocks, cfg);
    let is_async = normalized.is_some();
    let synchronous_concurrent = if is_async {
        None
    } else {
        normalize_concurrent_sync(&ins)
    };
    let is_concurrent =
        normalized.as_ref().is_some_and(|n| n.is_concurrent) || synchronous_concurrent.is_some();
    let exception_regions = if let Some(normalized) = normalized {
        let nop = crate::isa_gen::INSTRUCTIONS
            .iter()
            .find(|m| m.mnemonic == "nop")
            .unwrap();
        for index in normalized.erase {
            ins[index].meta = nop;
        }
        for index in normalized.jumps {
            // 仅替换操作码元数据，保留原指令的相对跳转编码与 PC。
            ins[index].meta = crate::isa_gen::INSTRUCTIONS
                .iter()
                .find(|m| m.mnemonic == "jmp" && m.size == ins[index].size)
                .unwrap();
        }
        normalized.regions
    } else {
        if let Some(erase) = synchronous_concurrent {
            let nop = crate::isa_gen::INSTRUCTIONS
                .iter()
                .find(|m| m.mnemonic == "nop")
                .unwrap();
            for index in erase {
                ins[index].meta = nop;
            }
        }
        code.try_blocks.clone()
    };
    let normalized_cfg;
    let cfg = if is_async {
        normalized_cfg = crate::decompile::cfg::build(&ins, bytes.len(), &exception_regions);
        &normalized_cfg
    } else {
        cfg
    };
    let mut regs: Vec<Option<Expr>> = vec![None; (code.num_vregs + code.num_args + 1) as usize];
    // 参数寄存器绑定
    for i in 0..code.num_args {
        regs[(code.num_vregs + i) as usize] = Some(Expr::Arg(i));
    }
    let hole_sentinel = ins
        .iter()
        .any(|i| i.meta.mnemonic == "ldhole")
        .then_some(code.num_vregs + code.num_args);
    let mut lifter = Lifter {
        file,
        method,
        module,
        ins: &ins,
        cfg,
        acc: Expr::Special("undefined"),
        acc_persisted: false,
        lex_names: Vec::new(),
        lexical_names: lexical_names.names,
        regs,
        stmts: Vec::new(),
        num_vregs: code.num_vregs,
        num_args: code.num_args,
        next_temp: code.num_vregs + code.num_args + u32::from(hole_sentinel.is_some()),
        active_loops: Vec::new(),
        exception_regions,
        active_regions: Vec::new(),
        is_async,
        finally_candidates: Vec::new(),
        hole_sentinel,
        super_receivers: proven_super_receivers(&ins, cfg, code, legacy, &lexical_names.this_loads),
        raw_fallback: if !is_async && ins.iter().any(|i| i.meta.mnemonic == "asyncfunctionenter") {
            Some("unrecognized async resume protocol")
        } else if ins.iter().any(|i| {
            matches!(
                i.meta.mnemonic,
                "creategeneratorobj" | "createasyncgeneratorobj"
            )
        }) {
            // 未验证的生成器状态机不能沿普通 CFG 执行，否则挂起和恢复结果会被丢失。
            Some("unrecognized generator resume protocol")
        } else {
            None
        },
    };
    lifter.run();
    LiftedBody {
        stmts: lifter.stmts,
        is_async,
        is_concurrent,
    }
}

/// 编译器异步协议的完整匹配结果，保留原始 PC 供异常表与调试表使用。
struct AsyncNormalization {
    erase: std::collections::HashSet<usize>,
    jumps: std::collections::HashSet<usize>,
    is_concurrent: bool,
    regions: Vec<crate::file::TryBlock>,
}

/// finally 的异常入口保存异常值，按哨兵调用清理块，最后区分正常离开与重抛。
/// 这里只接受编译器的显式协议；用户 catch 中恰好有类似调用不会被当作 finally。
fn finally_cleanup(ins: &[Ins], handler_pc: usize) -> Option<(usize, usize, u32, usize)> {
    #[derive(Clone, PartialEq)]
    enum Value {
        Reg(u32),
        Error,
        Undefined,
        Hole,
        Eq(Box<Value>, Box<Value>),
        Ne(Box<Value>, Box<Value>),
    }
    fn step(i: &Ins, acc: &mut Value, regs: &mut std::collections::HashMap<u32, Value>) -> bool {
        let operands = i.sig_values();
        let reg = operands
            .iter()
            .find(|(n, _, _)| n.starts_with('v'))
            .map(|(_, _, v)| *v as u32);
        match i.meta.mnemonic {
            "nop" => {}
            "sta" => {
                regs.insert(reg.unwrap_or(u32::MAX), acc.clone());
            }
            "lda" => {
                let r = reg.unwrap_or(u32::MAX);
                *acc = regs.get(&r).cloned().unwrap_or(Value::Reg(r));
            }
            "mov" | "mov.v16" if operands.len() == 2 => {
                let r = operands[1].2 as u32;
                regs.insert(
                    operands[0].2 as u32,
                    regs.get(&r).cloned().unwrap_or(Value::Reg(r)),
                );
            }
            "ldundefined" => *acc = Value::Undefined,
            "ldhole" => *acc = Value::Hole,
            "eq" | "noteq" => {
                let r = reg.unwrap_or(u32::MAX);
                let other = regs.get(&r).cloned().unwrap_or(Value::Reg(r));
                *acc = if i.meta.mnemonic == "eq" {
                    Value::Eq(Box::new(acc.clone()), Box::new(other))
                } else {
                    Value::Ne(Box::new(acc.clone()), Box::new(other))
                };
            }
            _ => return false,
        }
        true
    }
    let mut index = ins.iter().position(|i| i.pc == handler_pc)?;
    let mut acc = Value::Error;
    let mut regs = std::collections::HashMap::new();
    let mut steps = 0;
    while ins.get(index)?.meta.mnemonic != "jeqz" {
        if steps > 16 || !step(&ins[index], &mut acc, &mut regs) {
            return None;
        }
        index += 1;
        steps += 1;
    }
    let flag = match &acc {
        Value::Eq(a, b) => match (a.as_ref(), b.as_ref()) {
            (Value::Undefined, Value::Reg(r)) | (Value::Reg(r), Value::Undefined) => *r,
            _ => return None,
        },
        _ => return None,
    };
    let start = ins.get(index + 1)?.pc;
    let end = ins[index].jump_target()?;
    if end <= start {
        return None;
    }
    let exception_registers: Vec<_> = regs
        .iter()
        .filter_map(|(r, value)| (*value == Value::Error).then_some(*r))
        .collect();
    let linear_abrupt_cleanup = ins
        .iter()
        .filter(|i| i.pc >= start && i.pc < end)
        .all(|i| !i.meta.jump)
        && ins
            .iter()
            .rfind(|i| i.pc >= start && i.pc < end && i.meta.mnemonic != "nop")
            .is_some_and(|i| matches!(i.meta.mnemonic, "return" | "throw"));
    if !linear_abrupt_cleanup
        && ins
            .iter()
            .filter(|i| i.pc >= start && i.pc < end)
            .filter_map(written_register)
            .any(|r| exception_registers.contains(&r))
    {
        return None;
    }
    index = ins.iter().position(|i| i.pc == end)?;
    steps = 0;
    while ins.get(index)?.meta.mnemonic != "jeqz" {
        if steps > 16 || !step(&ins[index], &mut acc, &mut regs) {
            return None;
        }
        index += 1;
        steps += 1;
    }
    if !matches!(&acc, Value::Ne(a, b) if matches!((a.as_ref(), b.as_ref()), (Value::Error, Value::Hole) | (Value::Hole, Value::Error)))
    {
        return None;
    }
    let done = ins[index].jump_target()?;
    index += 1;
    while ins.get(index)?.pc < done && ins[index].meta.mnemonic != "throw" {
        if !step(&ins[index], &mut acc, &mut regs) {
            return None;
        }
        index += 1;
    }
    if ins.get(index)?.meta.mnemonic != "throw" || acc != Value::Error || ins[index].pc >= done {
        return None;
    }
    Some((start, end, flag, done))
}

fn written_register(instruction: &Ins) -> Option<u32> {
    if matches!(instruction.meta.mnemonic, "sta" | "mov" | "mov.v16") {
        instruction
            .sig_values()
            .into_iter()
            .find(|(n, _, _)| n.starts_with('v'))
            .map(|(_, _, r)| r as u32)
    } else {
        None
    }
}

/// 沿正常与异常后继检查覆盖前读取，供 finally 的临时寄存器保守证明使用。
fn register_live_from(ins: &[Ins], cfg: &Cfg, pc: usize, register: u32) -> bool {
    let Some(&entry) = cfg.pc_to_block.get(&pc) else {
        return false;
    };
    let mut pending = vec![entry];
    let mut visited = std::collections::HashSet::new();
    while let Some(block) = pending.pop() {
        if !visited.insert(block) {
            continue;
        }
        let block = &cfg.blocks[block];
        let mut killed = false;
        for instruction in &ins[block.start..block.end] {
            // 纯寄存器搬运不会在覆盖旧值前进入 catch；否则会误判循环快照为活跃输出。
            if !matches!(
                instruction.meta.mnemonic,
                "nop"
                    | "sta"
                    | "mov"
                    | "mov.v16"
                    | "lda"
                    | "ldundefined"
                    | "ldhole"
                    | "ldnull"
                    | "ldtrue"
                    | "ldfalse"
                    | "ldai"
                    | "fldai"
                    | "lda.str"
                    | "jmp"
                    | "jeqz"
                    | "jnez"
                    | "return"
                    | "returnundefined"
            ) {
                for (_, _, handler) in &block.try_of {
                    if let Some(h) = cfg.pc_to_block.get(&(*handler as usize)) {
                        pending.push(*h);
                    }
                }
            }
            let operands = instruction.sig_values();
            let written = written_register(instruction);
            let reads = operands
                .iter()
                .filter(|(n, _, _)| n.starts_with('v'))
                .enumerate()
                .any(|(n, (_, _, r))| !(n == 0 && written.is_some()) && *r as u32 == register);
            let range_read = instruction.meta.mnemonic.contains("range")
                && operands
                    .iter()
                    .rev()
                    .find(|(n, _, _)| n.starts_with('v'))
                    .is_some_and(|(_, _, first)| {
                        operands
                            .iter()
                            .rev()
                            .find(|(n, _, _)| n.starts_with("imm"))
                            .is_some_and(|(_, _, count)| {
                                *first <= u64::from(register)
                                    && u64::from(register) < first.saturating_add(*count + 1)
                            })
                    });
            if reads || range_read {
                return true;
            }
            if written == Some(register) {
                killed = true;
                break;
            }
        }
        if !killed {
            pending.extend(block.succs.iter().copied());
        }
    }
    false
}

/// 异步对象和 THROW 常量按正常/异常边传播；合流时保留所有可能值。
/// 寄存器在 catch 中可被复用，不能把曾保存异步对象的寄存器永久当成别名。
fn async_protocol_values(ins: &[Ins], cfg: &Cfg) -> Vec<Option<Vec<u8>>> {
    const MACHINE: u8 = 1;
    const ONE: u8 = 2;
    const OTHER: u8 = 4;
    let registers = ins
        .iter()
        .flat_map(Ins::sig_values)
        .filter(|(n, _, _)| n.starts_with('v'))
        .map(|(_, _, r)| r as usize + 1)
        .max()
        .unwrap_or(0);
    let mut states: Vec<Option<Vec<u8>>> = vec![None; ins.len()];
    if ins.is_empty() {
        return states;
    }
    states[0] = Some(vec![OTHER; registers + 1]);
    let mut pending = std::collections::VecDeque::from([0]);
    let pcs: std::collections::HashMap<_, _> =
        ins.iter().enumerate().map(|(i, x)| (x.pc, i)).collect();
    let mut blocks = vec![0; ins.len()];
    for (b, block) in cfg.blocks.iter().enumerate() {
        blocks[block.start..block.end].fill(b);
    }
    while let Some(index) = pending.pop_front() {
        let instruction = &ins[index];
        let before = states[index].as_ref().unwrap().clone();
        let mut after = before.clone();
        let operands = instruction.sig_values();
        let register = operands
            .iter()
            .find(|(n, _, _)| n.starts_with('v'))
            .map(|(_, _, r)| *r as usize);
        match instruction.meta.mnemonic {
            "asyncfunctionenter" => after[registers] = MACHINE,
            "ldai" => {
                after[registers] = if signed_imm_of(instruction) == 1 {
                    ONE
                } else {
                    OTHER
                }
            }
            "lda" => after[registers] = before[register.unwrap()],
            "sta" => after[register.unwrap()] = before[registers],
            "mov" | "mov.v16" => after[operands[0].2 as usize] = before[operands[1].2 as usize],
            _ if instruction.meta.acc_write => after[registers] = OTHER,
            _ => {}
        }
        let mut successors = Vec::new();
        if let Some(target) = instruction.jump_target()
            && let Some(&target) = pcs.get(&target)
        {
            successors.push((target, after.clone()));
        }
        if index + 1 < ins.len()
            && !instruction.meta.mnemonic.starts_with("jmp")
            && !matches!(
                instruction.meta.mnemonic,
                "return" | "returnundefined" | "throw"
            )
        {
            successors.push((index + 1, after));
        }
        // 纯搬运、常量与分支不会抛出；异常寄存器取抛出指令执行前的状态。
        if !matches!(
            instruction.meta.mnemonic,
            "nop"
                | "sta"
                | "lda"
                | "mov"
                | "mov.v16"
                | "ldai"
                | "fldai"
                | "ldundefined"
                | "ldhole"
                | "ldnull"
                | "ldtrue"
                | "ldfalse"
                | "lda.str"
                | "jmp"
                | "jeqz"
                | "jnez"
                | "return"
                | "returnundefined"
        ) {
            let mut exceptional = before;
            exceptional[registers] = OTHER;
            for (_, _, handler) in &cfg.blocks[blocks[index]].try_of {
                if let Some(&target) = pcs.get(&(*handler as usize)) {
                    successors.push((target, exceptional.clone()));
                }
            }
        }
        for (target, incoming) in successors {
            let changed = if let Some(current) = &mut states[target] {
                let mut changed = false;
                for (value, incoming) in current.iter_mut().zip(incoming) {
                    let merged = *value | incoming;
                    changed |= merged != *value;
                    *value = merged;
                }
                changed
            } else {
                states[target] = Some(incoming);
                true
            };
            if changed {
                pending.push_back(target);
            }
        }
    }
    states
}

/// 原生 super 的接收者固定为 this；交汇处必须所有路径都证明别名一致。
fn proven_super_receivers(
    ins: &[Ins],
    cfg: &Cfg,
    code: &CodeInfo,
    legacy: bool,
    this_loads: &std::collections::HashSet<usize>,
) -> std::collections::HashSet<usize> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let super_ops = [
        "ldsuperbyname",
        "ldsuperbyvalue",
        "stsuperbyname",
        "stsuperbyvalue",
    ];
    let base = |i: &Ins| {
        i.meta
            .mnemonic
            .strip_prefix("wide.")
            .unwrap_or(i.meta.mnemonic)
    };
    if legacy || code.num_args < 3 || !ins.iter().any(|i| super_ops.contains(&base(i))) {
        return HashSet::new();
    }
    let registers = (code.num_vregs + code.num_args) as usize;
    let mut states = vec![None::<Vec<bool>>; ins.len()];
    let mut initial = vec![false; registers + 1];
    initial[code.num_vregs as usize + 2] = true;
    states[0] = Some(initial);
    let mut pending = VecDeque::from([0]);
    let pcs: HashMap<_, _> = ins
        .iter()
        .enumerate()
        .map(|(index, i)| (i.pc, index))
        .collect();
    let mut blocks = vec![0; ins.len()];
    for (index, block) in cfg.blocks.iter().enumerate() {
        blocks[block.start..block.end].fill(index);
    }
    while let Some(index) = pending.pop_front() {
        let i = &ins[index];
        let before = states[index].as_ref().unwrap().clone();
        let mut after = before.clone();
        let operands = i.sig_values();
        let regs: Vec<_> = operands
            .iter()
            .filter(|(name, _, _)| name.starts_with('v'))
            .map(|(_, _, value)| *value as usize)
            .collect();
        // 损坏或不符合动态 ABI 的寄存器操作数不作别名推断。
        if regs.iter().any(|register| *register >= registers) {
            return HashSet::new();
        }
        match base(i) {
            "lda" => after[registers] = before[regs[0]],
            "sta" => after[regs[0]] = before[registers],
            "mov" | "mov.v16" => after[regs[0]] = before[regs[1]],
            "ldlexvar" => after[registers] = this_loads.contains(&i.pc),
            "ldthis" | "supercallthisrange" | "supercallarrowrange" | "supercallspread" => {
                after[registers] = true
            }
            _ if i.meta.acc_write => after[registers] = false,
            _ => {}
        }
        let mut successors = Vec::new();
        if let Some(target) = i.jump_target().and_then(|pc| pcs.get(&pc)) {
            successors.push((*target, after.clone()));
        }
        if index + 1 < ins.len()
            && !i.meta.mnemonic.starts_with("jmp")
            && !matches!(base(i), "return" | "returnundefined" | "throw")
        {
            successors.push((index + 1, after));
        }
        if !matches!(
            base(i),
            "nop"
                | "lda"
                | "sta"
                | "mov"
                | "mov.v16"
                | "ldai"
                | "fldai"
                | "lda.str"
                | "ldundefined"
                | "ldhole"
                | "ldnull"
                | "ldtrue"
                | "ldfalse"
                | "jmp"
                | "jeqz"
                | "jnez"
                | "return"
                | "returnundefined"
        ) {
            let mut exceptional = before;
            exceptional[registers] = false;
            for (_, _, handler) in &cfg.blocks[blocks[index]].try_of {
                if let Some(target) = pcs.get(&(*handler as usize)) {
                    successors.push((*target, exceptional.clone()));
                }
            }
        }
        for (target, incoming) in successors {
            let changed = if let Some(state) = &mut states[target] {
                let mut changed = false;
                for (current, incoming) in state.iter_mut().zip(incoming) {
                    let merged = *current && incoming;
                    changed |= merged != *current;
                    *current = merged;
                }
                changed
            } else {
                states[target] = Some(incoming);
                true
            };
            if changed {
                pending.push_back(target);
            }
        }
    }
    ins.iter()
        .enumerate()
        .filter_map(|(index, i)| {
            if !super_ops.contains(&base(i)) {
                return None;
            }
            let receiver = if base(i) == "ldsuperbyname" {
                registers
            } else {
                i.sig_values()
                    .into_iter()
                    .find(|(name, _, _)| name.starts_with('v'))?
                    .2 as usize
            };
            states[index]
                .as_ref()
                .is_some_and(|state| state[receiver])
                .then_some(i.pc)
        })
        .collect()
}

/// 编译器保存完成值、通知 taskpool、重新装载并返回的封闭尾序列。
fn concurrent_completion_tail(
    ins: &[Ins],
    significant: &[usize],
    position: usize,
) -> Option<Vec<usize>> {
    let notify = *significant.get(position)?;
    let ret = *significant.get(position + 1)?;
    if ins[notify].meta.mnemonic == "callruntime.notifyconcurrentresult"
        && ins[ret].meta.mnemonic == "return"
    {
        return Some(vec![notify, ret]);
    }
    let tail: [usize; 4] = significant.get(position..position + 4)?.try_into().ok()?;
    let [save, notify, reload, ret] = tail;
    let register = |index: usize| {
        ins[index]
            .sig_values()
            .into_iter()
            .find(|(n, _, _)| n.starts_with('v'))
            .map(|(_, _, r)| r as u32)
    };
    if ins[save].meta.mnemonic != "sta"
        || ins[notify].meta.mnemonic != "callruntime.notifyconcurrentresult"
        || ins[reload].meta.mnemonic != "lda"
        || register(save) != register(reload)
        || ins[ret].meta.mnemonic != "return"
        || ins[save + 1..reload]
            .iter()
            .any(|i| written_register(i) == register(save))
    {
        return None;
    }
    Some(tail.to_vec())
}

/// 同步并发函数仅接受所有普通返回都带终结通知的协议，禁止中途通知或绕过通知。
fn normalize_concurrent_sync(ins: &[Ins]) -> Option<std::collections::HashSet<usize>> {
    if ins.iter().any(|i| i.meta.mnemonic == "asyncfunctionenter") {
        return None;
    }
    let significant: Vec<_> = ins
        .iter()
        .enumerate()
        .filter(|(_, i)| !matches!(i.meta.mnemonic, "mov" | "mov.v16" | "nop"))
        .map(|(n, _)| n)
        .collect();
    let mut erase = std::collections::HashSet::new();
    let mut returns = std::collections::HashSet::new();
    let mut tail_entries = std::collections::HashSet::new();
    for (position, &notify) in significant.iter().enumerate() {
        if ins[notify].meta.mnemonic != "callruntime.notifyconcurrentresult" {
            continue;
        }
        let next = *significant.get(position + 1)?;
        let previous = *significant.get(position.checked_sub(1)?)?;
        let ret = match ins[next].meta.mnemonic {
            "returnundefined" if ins[previous].meta.mnemonic == "ldundefined" => next,
            "return" => next,
            "lda" => {
                let register = written_register(&ins[previous]).or_else(|| {
                    (ins[previous].meta.mnemonic == "lda")
                        .then(|| ins[previous].sig_values()[0].2 as u32)
                })?;
                if ins[next].sig_values()[0].2 != u64::from(register)
                    || ins[previous + 1..next]
                        .iter()
                        .any(|i| written_register(i) == Some(register))
                {
                    return None;
                }
                let ret = *significant.get(position + 2)?;
                if ins[ret].meta.mnemonic != "return" {
                    return None;
                }
                tail_entries.insert(next);
                ret
            }
            _ => return None,
        };
        erase.insert(notify);
        returns.insert(ret);
        tail_entries.insert(ret);
    }
    if erase.is_empty() {
        return None;
    }
    for (index, instruction) in ins.iter().enumerate() {
        if matches!(instruction.meta.mnemonic, "return" | "returnundefined")
            && !returns.contains(&index)
        {
            return None;
        }
        if instruction.jump_target().is_some_and(|target| {
            ins.iter()
                .position(|i| i.pc == target)
                .is_some_and(|i| tail_entries.contains(&i))
        }) {
            return None;
        }
    }
    Some(erase)
}

/// 折叠普通 async 函数的挂起/恢复协议；生成器或未知恢复模式不猜测。
/// 优化器插入的寄存器快照保持原位，只删除经过逐指令值流证明的异步对象搬运。
fn normalize_async(
    ins: &[Ins],
    regions: &[crate::file::TryBlock],
    cfg: &Cfg,
) -> Option<AsyncNormalization> {
    let entries: Vec<_> = ins
        .iter()
        .enumerate()
        .filter(|(_, x)| x.meta.mnemonic == "asyncfunctionenter")
        .collect();
    let [(enter, _)] = entries.as_slice() else {
        return None;
    };
    let enter = *enter;
    let values = async_protocol_values(ins, cfg);
    let reg = |i: usize| {
        ins.get(i)?
            .sig_values()
            .into_iter()
            .find(|(n, _, _)| n.starts_with('v'))
            .map(|(_, _, r)| r)
    };
    let machine = |i: usize| {
        reg(i).is_some_and(|r| {
            values
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|v| v[r as usize] == 1)
        })
    };
    let mn = |i: usize| ins.get(i).map(|x| x.meta.mnemonic);
    if mn(enter + 1) != Some("sta") {
        return None;
    }
    let mut erase = std::collections::HashSet::from([enter, enter + 1]);
    let mut accepted = erase.clone();
    let mut jumps = std::collections::HashSet::new();
    let mut concurrent_tails = std::collections::HashSet::new();
    // mov 不改累加器，可穿插在协议内；普通值快照仍交给提升器处理。
    let significant: Vec<_> = ins
        .iter()
        .enumerate()
        .filter(|(_, x)| !matches!(x.meta.mnemonic, "mov" | "mov.v16" | "nop"))
        .map(|(i, _)| i)
        .collect();
    for (i, instruction) in ins.iter().enumerate() {
        if matches!(instruction.meta.mnemonic, "mov" | "mov.v16") {
            let operands = instruction.sig_values();
            if values[i]
                .as_ref()
                .is_some_and(|v| v[operands[1].2 as usize] == 1)
            {
                accepted.insert(i);
                erase.insert(i);
            }
        }
    }
    let outer: Vec<_> = regions
        .iter()
        .enumerate()
        .filter_map(|(n, tb)| {
            let [cb] = tb.catch_blocks.as_slice() else {
                return None;
            };
            if cb.type_idx != 0 || cb.handler_pc != tb.start_pc + tb.length {
                return None;
            }
            let h = significant
                .iter()
                .position(|&i| ins[i].pc == cb.handler_pc as usize)?;
            let get = |k: usize| significant.get(h + k).copied();
            let first = get(0)?;
            let reload = usize::from(mn(get(1)?) == Some("lda") && reg(first) == reg(get(1)?));
            let reject = get(1 + reload)?;
            let completion = h + 2 + reload;
            let tail = concurrent_completion_tail(ins, &significant, completion);
            let (ret, end) = if let Some(tail) = &tail {
                (*tail.last()?, completion + tail.len())
            } else {
                (*significant.get(completion)?, completion + 1)
            };
            if mn(first) == Some("sta")
                && mn(reject) == Some("asyncfunctionreject")
                && machine(reject)
                && mn(ret) == Some("return")
                && end == significant.len()
            {
                Some((n, first, tail))
            } else {
                None
            }
        })
        .collect();
    let [(outer_region, reject, reject_tail)] = outer.as_slice() else {
        return None;
    };
    let is_concurrent = reject_tail.is_some();
    if let Some(tail) = reject_tail {
        concurrent_tails.extend(tail.iter().copied());
    }
    erase.extend(*reject..ins.len());
    accepted.extend(*reject..ins.len());
    for (position, &i) in significant
        .iter()
        .enumerate()
        .take_while(|(_, i)| **i < *reject)
    {
        if values[i].is_none() {
            continue;
        }
        match ins[i].meta.mnemonic {
            "asyncfunctionawaituncaught" => {
                let protocol = significant.get(position..position + 13)?;
                let at = |n: usize| protocol[n];
                let expected = [
                    "asyncfunctionawaituncaught",
                    "suspendgenerator",
                    "lda",
                    "resumegenerator",
                    "sta",
                    "lda",
                    "getresumemode",
                    "sta",
                    "",
                    "eq",
                    "jeqz",
                    "lda",
                    "throw",
                ];
                if expected
                    .iter()
                    .enumerate()
                    .any(|(n, &expected)| n != 8 && mn(at(n)) != Some(expected))
                    || ![0, 1, 2, 5].into_iter().all(|n| machine(at(n)))
                    || reg(at(9)) != reg(at(7))
                    || reg(at(11)) != reg(at(4))
                {
                    return None;
                }
                let target = ins[at(10)].jump_target()?;
                let continuation = significant.get(position + 13).copied()?;
                // 优化器可把正常恢复边穿透到分支尾部或循环头；恢复后仍需保留该边。
                if !ins.iter().any(|x| x.pc == target) {
                    return None;
                }
                if target <= ins[at(12)].pc || target > ins[continuation].pc {
                    jumps.insert(at(10));
                }
                let constant = match mn(at(8)) {
                    Some("ldai") => signed_imm_of(&ins[at(8)]) == 1,
                    Some("lda") => values[at(8)].as_ref()?.get(reg(at(8))? as usize) == Some(&2),
                    _ => false,
                };
                if !constant {
                    return None;
                }
                // 插入的快照不能改写恢复结果或比较模式，模式也不能逃逸到用户代码。
                let result = reg(at(4))? as u32;
                let mode = reg(at(7))? as u32;
                for (begin, end, register) in
                    [(at(4) + 1, at(11), result), (at(7) + 1, at(9), mode)]
                {
                    if ins[begin..end]
                        .iter()
                        .any(|x| written_register(x) == Some(register))
                    {
                        return None;
                    }
                }
                if ins[at(7) + 1..=at(12)].iter().any(|x| {
                    matches!(x.meta.mnemonic, "mov" | "mov.v16")
                        && x.sig_values()
                            .get(1)
                            .is_some_and(|(_, _, r)| *r == u64::from(mode))
                }) || register_live_from(ins, cfg, target, mode)
                {
                    return None;
                }
                accepted.extend(protocol.iter().copied());
                erase.extend([at(1), at(2), at(3)]);
                erase.extend(protocol[5..].iter().copied());
            }
            "asyncfunctionresolve" => {
                if !machine(i) {
                    return None;
                }
                let ret = if is_concurrent {
                    let tail = concurrent_completion_tail(ins, &significant, position + 1)?;
                    accepted.extend(tail.iter().copied());
                    erase.extend(tail[..tail.len() - 1].iter().copied());
                    concurrent_tails.extend(tail.iter().copied());
                    *tail.last()?
                } else {
                    *significant.get(position + 1)?
                };
                if mn(ret) != Some("return") {
                    return None;
                }
                accepted.extend([i, ret]);
                erase.insert(i);
            }
            _ => {}
        }
    }
    for (i, instruction) in ins.iter().enumerate() {
        if values[i].is_none() {
            continue;
        }
        let protocol = matches!(
            instruction.meta.mnemonic,
            "asyncfunctionenter"
                | "asyncfunctionawaituncaught"
                | "asyncfunctionresolve"
                | "asyncfunctionreject"
                | "suspendgenerator"
                | "resumegenerator"
                | "getresumemode"
                | "callruntime.notifyconcurrentresult"
                | "return"
                | "returnundefined"
        );
        let written = written_register(instruction);
        let generator_use = values[i].as_ref().is_some_and(|values| {
            instruction
                .sig_values()
                .iter()
                .enumerate()
                .any(|(n, (name, _, r))| {
                    name.starts_with('v')
                        && !(n == 0 && written.is_some())
                        && values[*r as usize] & 1 != 0
                })
                || (instruction.meta.mnemonic.contains("range")
                    && instruction
                        .sig_values()
                        .iter()
                        .rev()
                        .find(|(n, _, _)| n.starts_with('v'))
                        .is_some_and(|(_, _, first)| {
                            instruction
                                .sig_values()
                                .iter()
                                .rev()
                                .find(|(n, _, _)| n.starts_with("imm"))
                                .is_some_and(|(_, _, count)| {
                                    values[..values.len() - 1].iter().enumerate().any(
                                        |(r, value)| {
                                            *value & 1 != 0
                                                && *first <= r as u64
                                                && (r as u64) < first.saturating_add(*count + 1)
                                        },
                                    )
                                })
                        }))
                || (instruction.meta.acc_read
                    && values[values.len() - 1] & 1 != 0
                    && !matches!(
                        instruction
                            .meta
                            .mnemonic
                            .strip_prefix("wide.")
                            .unwrap_or(instruction.meta.mnemonic),
                        "nop"
                            | "mov"
                            | "mov.v16"
                            | "lda"
                            | "ldai"
                            | "fldai"
                            | "lda.str"
                            | "ldundefined"
                            | "ldhole"
                            | "ldnull"
                            | "ldtrue"
                            | "ldfalse"
                            | "ldfunction"
                            | "ldnewtarget"
                            | "ldthis"
                            | "ldnan"
                            | "ldinfinity"
                            | "ldsymbol"
                            | "ldglobal"
                            | "tryldglobalbyname"
                            | "ldglobalvar"
                            | "ldexternalmodulevar"
                            | "ldlocalmodulevar"
                            | "getmodulenamespace"
                            | "ldlexvar"
                            | "wide.ldlexvar"
                            | "createemptyarray"
                            | "createemptyobject"
                            | "createarraywithbuffer"
                            | "createobjectwithbuffer"
                            | "definefunc"
                            | "newlexenv"
                            | "wide.newlexenv"
                            | "newlexenvwithname"
                            | "wide.newlexenvwithname"
                            | "newobjrange"
                            | "wide.newobjrange"
                            | "copyrestargs"
                            | "wide.copyrestargs"
                            | "callruntime.ldlazymodulevar"
                    ))
        });
        if (protocol || generator_use) && !accepted.contains(&i) {
            return None;
        }
        if instruction.jump_target().is_some_and(|target| {
            ins.iter()
                .position(|x| x.pc == target)
                .is_some_and(|target| concurrent_tails.contains(&target))
        }) {
            return None;
        }
        if !accepted.contains(&i)
            && instruction.jump_target().is_some_and(|target| {
                ins.iter()
                    .position(|x| x.pc == target)
                    .is_some_and(|target| {
                        concurrent_tails.contains(&target)
                            || (erase.contains(&target)
                                && !matches!(
                                    ins[target].meta.mnemonic,
                                    "mov" | "mov.v16" | "nop" | "asyncfunctionresolve"
                                ))
                    })
            })
        {
            return None;
        }
    }
    Some(AsyncNormalization {
        erase,
        jumps,
        is_concurrent,
        regions: regions
            .iter()
            .enumerate()
            .filter(|(n, _)| n != outer_region)
            .map(|(_, r)| r.clone())
            .collect(),
    })
}

impl<'a> Lifter<'a> {
    fn v(&self, n: u64) -> Expr {
        self.v_target(n)
    }

    fn set_v(&mut self, n: u64, e: Expr) {
        if n as usize >= self.regs.len() {
            self.regs.resize(n as usize + 1, None);
        }
        self.regs[n as usize] = Some(e);
    }

    /// 属性写入不清除累加器；先保存计算结果，避免返回时重新执行调用/读取。
    fn persist_value(&mut self) -> Expr {
        if !matches!(
            self.acc,
            Expr::Reg(_)
                | Expr::Arg(_)
                | Expr::Int(_)
                | Expr::Float(_)
                | Expr::Str(_)
                | Expr::Bool(_)
                | Expr::Special(_)
        ) {
            let temp = self.next_temp;
            self.next_temp += 1;
            let value = self.take_acc();
            self.stmts.push(Stmt::Expr(Expr::Assign(
                Box::new(Expr::Reg(temp)),
                Box::new(value),
            )));
            self.acc = Expr::Reg(temp);
        }
        self.acc_persisted = true;
        self.peek_acc()
    }

    /// 字段定义保留累加器，并绕过接收者继承链中的 setter。
    fn define_field(&mut self, receiver: Expr, key: Expr) {
        let value = self.persist_value();
        self.stmts
            .push(Stmt::Expr(define_data_property(receiver, key, value)));
    }

    /// 对象展开返回原目标，按 VM 的描述符复制协议保留键顺序与访问器。
    fn copy_data_properties(&mut self, receiver: Expr) {
        let source_value = self.take_acc();
        let source = self.next_temp;
        let target = source + 1;
        let descriptor = source + 2;
        self.next_temp += 3;
        for (reg, value) in [(source, source_value), (target, receiver)] {
            self.stmts.push(Stmt::Expr(Expr::Assign(
                Box::new(Expr::Reg(reg)),
                Box::new(value),
            )));
        }
        let key = format!("spreadKey{source}");
        let desc = Expr::Reg(descriptor);
        let assign = |target, value| Stmt::Expr(Expr::Assign(Box::new(target), Box::new(value)));
        let not_nullish = Expr::Bin(
            "&&",
            Box::new(Expr::Bin(
                "!==",
                Box::new(Expr::Reg(source)),
                Box::new(Expr::Special("null")),
            )),
            Box::new(Expr::Bin(
                "!==",
                Box::new(Expr::Reg(source)),
                Box::new(Expr::Special("undefined")),
            )),
        );
        self.stmts.push(Stmt::If(
            not_nullish,
            vec![
                assign(
                    Expr::Reg(source),
                    call_expr(Expr::Special("globalThis.Object"), vec![Expr::Reg(source)]),
                ),
                Stmt::ForOf(
                    key.clone(),
                    call_expr(
                        Expr::Special("globalThis.Reflect.ownKeys"),
                        vec![Expr::Reg(source)],
                    ),
                    vec![
                        assign(
                            desc.clone(),
                            call_expr(
                                Expr::Special("globalThis.Object.getOwnPropertyDescriptor"),
                                vec![Expr::Reg(source), Expr::Name(key.clone())],
                            ),
                        ),
                        Stmt::If(
                            Expr::Bin(
                                "&&",
                                Box::new(Expr::Bin(
                                    "!==",
                                    Box::new(desc.clone()),
                                    Box::new(Expr::Special("undefined")),
                                )),
                                Box::new(Expr::Member(Box::new(desc.clone()), "enumerable".into())),
                            ),
                            vec![
                                // RuntimeCopyDataProperties 复制访问器本身，不读取 getter。
                                // JS 描述符不能同时带 writable 与 get/set；仅数据属性添加 writable。
                                Stmt::If(
                                    Expr::Bin(
                                        "in",
                                        Box::new(Expr::Str("value".into())),
                                        Box::new(desc.clone()),
                                    ),
                                    vec![assign(
                                        Expr::Member(Box::new(desc.clone()), "writable".into()),
                                        Expr::Bool(true),
                                    )],
                                    vec![],
                                ),
                                assign(
                                    Expr::Member(Box::new(desc.clone()), "configurable".into()),
                                    Expr::Bool(true),
                                ),
                                // VM 保留 DefineOwnProperty 的异常，但忽略 false 返回值。
                                Stmt::Expr(call_expr(
                                    Expr::Special("globalThis.Reflect.defineProperty"),
                                    vec![Expr::Reg(target), Expr::Name(key), desc],
                                )),
                            ],
                            vec![],
                        ),
                    ],
                ),
            ],
            vec![],
        ));
        self.acc = Expr::Reg(target);
        self.acc_persisted = true;
    }

    /// 计算属性方法写入后设置函数名；键转换与函数求值各执行一次。
    fn store_own_with_name(&mut self, receiver: Expr, key: Expr) {
        let value = self.persist_value();
        let converted = self.next_temp;
        self.next_temp += 1;
        self.stmts.push(Stmt::Expr(Expr::Assign(
            Box::new(Expr::Reg(converted)),
            Box::new(to_property_key(key)),
        )));
        self.stmts.push(Stmt::Expr(define_data_property(
            receiver,
            Expr::Reg(converted),
            value.clone(),
        )));
        // 模板仅包含固定运行时辅助函数，操作数仍作为 AST 参数参与数据流分析。
        self.stmts.push(Stmt::Expr(call_expr(Expr::TemplateLit(
            "((value, key) => { if (typeof value === \"function\") { const name = typeof key === \"symbol\" ? (key.description === undefined ? \"\" : \"[\" + key.description + \"]\") : key; globalThis.Object.defineProperty(value, \"name\", { value: name, writable: false, enumerable: false, configurable: true }); } })".into()
        ), vec![value, Expr::Reg(converted)])));
    }

    fn take_acc(&mut self) -> Expr {
        self.acc_persisted = false;
        std::mem::replace(&mut self.acc, Expr::Special("undefined"))
    }

    /// 读取 acc 副本（Panda ISA 中 st* 类指令不清除 acc）
    fn peek_acc(&self) -> Expr {
        self.acc.clone()
    }

    /// 覆盖 acc 前调用：若当前 acc 含调用副作用且尚未消费，补发语句
    fn set_acc(&mut self, e: Expr) {
        let was_persisted = self.acc_persisted;
        let old = std::mem::replace(&mut self.acc, e);
        self.acc_persisted = false;
        if has_effect(&old) && !was_persisted && !matches!(old, Expr::Special(_)) {
            self.stmts.push(Stmt::Expr(old));
        }
    }

    /// 迭代器与 next/return 的结果必须是对象（函数也属于对象），不能省略运行时检查。
    fn require_object(&mut self, value: Expr) {
        self.persist_value();
        let is_null = Expr::Bin(
            "===",
            Box::new(value.clone()),
            Box::new(Expr::Special("null")),
        );
        let type_check = |kind: &str| {
            Expr::Bin(
                "!==",
                Box::new(Expr::Un("typeof", Box::new(value.clone()))),
                Box::new(Expr::Str(kind.into())),
            )
        };
        let condition = Expr::Bin(
            "||",
            Box::new(is_null),
            Box::new(Expr::Bin(
                "&&",
                Box::new(type_check("object")),
                Box::new(type_check("function")),
            )),
        );
        self.stmts.push(Stmt::If(
            condition,
            vec![Stmt::Throw(Expr::Call {
                callee: Box::new(Expr::Name("TypeError".into())),
                args: vec![Expr::Str("Iterator result is not an object".into())],
                new: true,
                this_arg: None,
            })],
            vec![],
        ));
    }

    fn imm(&self, i: &Ins) -> u64 {
        i.field("imm").or_else(|| i.field("imm1")).unwrap_or(0)
    }

    /// 最后一个 imm 字段（如 stobjbyindex 的 imm2）
    fn imm_last(&self, i: &Ins) -> u64 {
        let mut last = 0;
        for (n, _, v) in i.sig_values() {
            if n.starts_with("imm") {
                last = v;
            }
        }
        last
    }

    fn reg_of(&self, i: &Ins) -> u64 {
        self.regs_of(i).first().copied().unwrap_or(0)
    }

    /// sig 中全部 v 字段
    fn regs_of(&self, i: &Ins) -> Vec<u64> {
        i.sig_values()
            .iter()
            .filter(|(n, _, _)| n.starts_with('v'))
            .map(|(_, _, v)| *v)
            .collect()
    }

    /// 模块导入绑定名（ldexternalmodulevar idx）
    fn module_import_name(&self, idx: u64) -> String {
        let idx = idx as usize;
        external_import_name(self.module, idx)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("imported_{idx}"))
    }

    /// callruntime.ldlazymodulevar 使用与普通外部模块变量相同的导入槽位。
    fn lazy_module_import_name(&self, idx: u64) -> String {
        let idx = idx as usize;
        external_import_name(self.module, idx)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("lazy_imported_{idx}"))
    }

    /// getmodulenamespace 的立即数是 requests/module_idx，不是 namespace_imports 的数组索引。
    fn module_namespace_name(&self, request: u64) -> Option<&str> {
        namespace_name_for_request(self.module, request)
    }

    /// 本地导出绑定名（ldlocalmodulevar / stmodulevar idx）
    fn module_local_name(&self, idx: u64) -> String {
        let idx = idx as usize;
        if let Some(m) = self.module
            && let Some(l) = m.local_exports.get(idx)
        {
            return l.local_name.clone();
        }
        format!("exported_{idx}")
    }

    fn module_export_name(&self, idx: u64) -> String {
        let idx = idx as usize;
        if let Some(m) = self.module
            && let Some(l) = m.local_exports.get(idx)
        {
            return l.export_name.clone();
        }
        format!("exported_{idx}")
    }

    fn resolve_str(&self, i: &Ins) -> String {
        for (name, _decl, val) in i.sig_values() {
            if name.ends_with("string_id") {
                return self
                    .file
                    .resolve_msl(self.method.offset, val as u32)
                    .ok()
                    .and_then(|off| self.file.string_at(off).ok())
                    .unwrap_or_else(|| format!("str@{val}"));
            }
            let _ = name;
        }
        String::new()
    }

    fn resolve_method_name(&self, i: &Ins) -> Option<(u32, String)> {
        for (name, _decl, val) in i.sig_values() {
            if name.ends_with("method_id") {
                let off = self.file.resolve_msl(self.method.offset, val as u32).ok()?;
                let mut r = crate::reader::Reader::new(self.file.bytes());
                r.seek(off as usize).ok()?;
                let _c = r.u16().ok()?;
                let _p = r.u16().ok()?;
                let name_off = r.u32().ok()?;
                let name = self.file.string_at(name_off).ok()?;
                return Some((off, name));
            }
        }
        None
    }

    fn litarr_of(&self, i: &Ins) -> Option<u32> {
        for (name, _decl, val) in i.sig_values() {
            if name.ends_with("literalarray_id") {
                return self.file.resolve_msl(self.method.offset, val as u32).ok();
            }
        }
        None
    }

    fn fallback(&mut self, i: &Ins) {
        let ops = i
            .sig_values()
            .iter()
            .map(|(n, _, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.stmts
            .push(Stmt::Comment(format!("{} {}", i.mnemonic(), ops)));
    }

    fn run(&mut self) {
        if self.raw_fallback.is_some() {
            self.emit_raw_fallback();
            return;
        }
        // JD arkdecompiler 式区域发射：块级语句 + 后继胶水。
        // 汇合块 = 多前驱块；循环 = 自然循环（支配者保证）
        let mut visited = vec![false; self.cfg.blocks.len()];
        let no_stop: Vec<usize> = Vec::new();
        let mut stmts = self.emit_seq(0, &no_stop, &mut visited);
        crate::decompile::structure::recover_finally(&mut stmts, &self.finally_candidates);
        if self.raw_fallback.is_some() {
            if let Some(recovered) = self.emit_exception_cfg() {
                self.stmts = recovered;
                self.prepend_hole_sentinel();
                return;
            }
            self.emit_raw_fallback();
            return;
        }
        // 未被结构化覆盖的处理器不可悄悄丢弃；保留范围与处理器语句供人工核对。
        for region in self.exception_regions.clone() {
            for catch in region.catch_blocks {
                if let Some(&handler) = self.cfg.pc_to_block.get(&(catch.handler_pc as usize))
                    && !visited[handler]
                {
                    let mut diagnostic = format!(
                        "unstructured exception handler: try {:#x}..{:#x}, handler {:#x}, type {}",
                        region.start_pc,
                        region.start_pc + region.length,
                        catch.handler_pc,
                        catch.type_idx
                    );
                    let end = if catch.code_size == 0 {
                        usize::MAX
                    } else {
                        (catch.handler_pc + catch.code_size) as usize
                    };
                    for instruction in self
                        .ins
                        .iter()
                        .filter(|i| i.pc >= catch.handler_pc as usize && i.pc < end)
                    {
                        diagnostic.push_str(&format!(
                            "\n{:#x}: {} {:?}",
                            instruction.pc,
                            instruction.mnemonic(),
                            instruction.sig_values()
                        ));
                    }
                    // 诊断置于入口，既不伪造可执行 catch，也不会在 return 后被整理丢弃。
                    stmts.insert(0, Stmt::Comment(diagnostic));
                    visited[handler] = true;
                }
            }
        }
        std::mem::swap(&mut stmts, &mut self.stmts);
        self.prepend_hole_sentinel();
        // run 结束：self.stmts 已包含全部语句
    }

    /// 只有实际使用 hole 的函数才建立私有标记；每次调用都有独立对象身份。
    fn prepend_hole_sentinel(&mut self) {
        if let Some(temp) = self.hole_sentinel {
            self.stmts.insert(
                0,
                Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Reg(temp)),
                    Box::new(Expr::Object(Vec::new())),
                )),
            );
        }
    }

    /// mov 不修改 VM 累加器，写入其表达式依赖的槽位前必须保存旧值。
    fn snapshot_acc_before_register_write(&mut self, register: u64) {
        fn reads(expr: &Expr, target: &Expr) -> bool {
            match (expr, target) {
                (Expr::Reg(a), Expr::Reg(b)) | (Expr::Arg(a), Expr::Arg(b)) => a == b,
                (Expr::Assign(left, right), _) => {
                    reads(right, target)
                        || (!matches!(left.as_ref(), Expr::Reg(_) | Expr::Arg(_))
                            && reads(left, target))
                }
                (Expr::Bin(_, a, b) | Expr::Index(a, b), _) => reads(a, target) || reads(b, target),
                (
                    Expr::Un(_, e)
                    | Expr::Await(e)
                    | Expr::Update(_, e, _)
                    | Expr::Member(e, _)
                    | Expr::Spread(e),
                    _,
                ) => reads(e, target),
                (Expr::Cond(c, a, b), _) => {
                    reads(c, target) || reads(a, target) || reads(b, target)
                }
                (
                    Expr::Call {
                        callee,
                        args,
                        this_arg,
                        ..
                    },
                    _,
                ) => {
                    reads(callee, target)
                        || args.iter().any(|e| reads(e, target))
                        || this_arg.as_ref().is_some_and(|e| reads(e, target))
                }
                (Expr::Object(properties), _) => properties.iter().any(|(_, e)| reads(e, target)),
                (Expr::Array(elements), _) => elements.iter().any(|e| reads(e, target)),
                _ => false,
            }
        }
        if reads(&self.acc, &self.v_target(register))
            || (!self.acc_persisted && has_effect(&self.acc))
        {
            let temp = self.next_temp;
            self.next_temp += 1;
            let value = self.take_acc();
            self.stmts.push(Stmt::Expr(Expr::Assign(
                Box::new(Expr::Reg(temp)),
                Box::new(value),
            )));
            self.acc = Expr::Reg(temp);
            self.acc_persisted = true;
        }
    }

    /// 不能安全嵌套的异常 CFG 使用显式块状态，不跨保护边界搬移求值。
    /// 每个块只受其最内层 catchall 保护；处理器本身在下一轮按自己的覆盖执行。
    fn emit_exception_cfg(&mut self) -> Option<Vec<Stmt>> {
        if !matches!(
            self.raw_fallback,
            Some(
                "multiple unresolved exception-region exits"
                    | "unverified normal exception-handler entry"
                    | "unverified shared finally continuation"
            )
        ) || self.cfg.blocks.is_empty()
            || self.exception_regions.iter().any(|r| {
                r.catch_blocks.len() != 1
                    || r.catch_blocks[0].type_idx != 0
                    || !self
                        .cfg
                        .pc_to_block
                        .contains_key(&(r.catch_blocks[0].handler_pc as usize))
            })
        {
            return None;
        }
        self.raw_fallback = None;
        self.stmts.clear();
        self.next_temp = self.num_vregs + self.num_args;
        self.hole_sentinel = None;
        self.active_loops.clear();
        self.active_regions.clear();
        self.finally_candidates.clear();
        self.lex_names.clear();
        let state = self.next_temp;
        let accumulator = state + 1;
        self.next_temp += 2;
        if self.ins.iter().any(|i| i.meta.mnemonic == "ldhole") {
            self.hole_sentinel = Some(self.next_temp);
            self.next_temp += 1;
        }
        // 规范化后的异步拒绝胶水可能留下无后继的不可达块，不能把这些
        // 原本不会执行的指令作为恢复失败，也不能发射为可选择的状态。
        let mut reachable = std::collections::HashSet::new();
        let mut pending = vec![0];
        while let Some(block) = pending.pop() {
            if !reachable.insert(block) {
                continue;
            }
            pending.extend(self.cfg.blocks[block].succs.iter().copied());
            if let Some(region) = self
                .exception_regions
                .iter()
                .filter(|r| {
                    self.cfg.blocks[block].start_pc >= r.start_pc as usize
                        && self.cfg.blocks[block].end_pc <= (r.start_pc + r.length) as usize
                })
                .min_by_key(|r| r.length)
            {
                pending.push(self.cfg.pc_to_block[&(region.catch_blocks[0].handler_pc as usize)]);
            }
        }
        let mut cases = Vec::new();
        for bi in 0..self.cfg.blocks.len() {
            if !reachable.contains(&bi) {
                continue;
            }
            self.regs.fill(None);
            for argument in 0..self.num_args {
                self.regs[(self.num_vregs + argument) as usize] = Some(Expr::Arg(argument));
            }
            self.acc = Expr::Reg(accumulator);
            self.acc_persisted = true;
            let mut body = Vec::new();
            self.emit_block_insns(bi, &mut body);
            if body.iter().any(|s| matches!(s, Stmt::Comment(_))) {
                self.raw_fallback = Some("unsupported instruction in exception CFG");
                return None;
            }
            let (target, mnemonic, conditional) = self.tail_of(bi);
            if !self.ends_in_return(bi) {
                body.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Reg(accumulator)),
                    Box::new(self.peek_acc()),
                )));
                let successor = if let Some(target) = target {
                    if conditional {
                        let fallthrough = bi + 1;
                        if !self.cfg.blocks[bi].succs.contains(&fallthrough) {
                            self.raw_fallback = Some("invalid conditional edge in exception CFG");
                            return None;
                        }
                        Expr::Cond(
                            Box::new(self.branch_cond(&mnemonic)),
                            Box::new(Expr::Int(target as i64)),
                            Box::new(Expr::Int(fallthrough as i64)),
                        )
                    } else {
                        Expr::Int(target as i64)
                    }
                } else if let Some(next) = self.cfg.blocks[bi].succs.first() {
                    Expr::Int(*next as i64)
                } else {
                    self.raw_fallback = Some("unterminated block in exception CFG");
                    return None;
                };
                body.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Reg(state)),
                    Box::new(successor),
                )));
                body.push(Stmt::Continue);
            }
            let protection = self
                .exception_regions
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    self.cfg.blocks[bi].start_pc >= r.start_pc as usize
                        && self.cfg.blocks[bi].end_pc <= (r.start_pc + r.length) as usize
                })
                .min_by_key(|(order, r)| (r.length, *order));
            if let Some((index, region)) = protection {
                let handler = self.cfg.pc_to_block[&(region.catch_blocks[0].handler_pc as usize)];
                let error = format!("error{index}");
                body = vec![Stmt::Try(
                    body,
                    vec![(
                        Some(error.clone()),
                        vec![
                            Stmt::Expr(Expr::Assign(
                                Box::new(Expr::Reg(accumulator)),
                                Box::new(Expr::Name(error)),
                            )),
                            Stmt::Expr(Expr::Assign(
                                Box::new(Expr::Reg(state)),
                                Box::new(Expr::Int(handler as i64)),
                            )),
                            Stmt::Continue,
                        ],
                    )],
                )];
            }
            cases.push(Stmt::If(
                Expr::Bin(
                    "===",
                    Box::new(Expr::Reg(state)),
                    Box::new(Expr::Int(bi as i64)),
                ),
                body,
                Vec::new(),
            ));
        }
        cases.push(Stmt::Break);
        Some(vec![
            Stmt::Expr(Expr::Assign(
                Box::new(Expr::Reg(state)),
                Box::new(Expr::Int(0)),
            )),
            Stmt::Expr(Expr::Assign(
                Box::new(Expr::Reg(accumulator)),
                Box::new(Expr::Special("undefined")),
            )),
            Stmt::While(Expr::Bool(true), cases),
        ])
    }

    /// 未证明的协议不输出貌似可执行的残缺函数；原始指令和异常表完整保留。
    fn emit_raw_fallback(&mut self) {
        let reason = self.raw_fallback.unwrap_or("unsupported control flow");
        let assembly = self
            .method
            .code
            .as_ref()
            .and_then(|code| {
                crate::disasm::disassemble_method(
                    &crate::disasm::DisasmCtx {
                        file: self.file,
                        method_off: self.method.offset,
                    },
                    &self.method.name,
                    code.instructions(&self.file.data),
                    code.num_vregs,
                    code.num_args,
                    &code.try_blocks,
                )
                .ok()
            })
            .unwrap_or_else(|| {
                self.ins
                    .iter()
                    .map(|i| format!("{:#x}: {} {:?}", i.pc, i.mnemonic(), i.sig_values()))
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        self.stmts = vec![Stmt::Comment(format!(
            "Conservative fallback: {reason}. Original bytecode and exception handlers:\n{}",
            assembly.replace("*/", "* /")
        ))];
    }

    /// 从 entry 顺序发射直到 stop 集合或已访问块
    fn emit_seq(&mut self, entry: usize, stop: &[usize], visited: &mut Vec<bool>) -> Vec<Stmt> {
        let mut out: Vec<Stmt> = Vec::new();
        let mut bi = entry;
        loop {
            if bi >= self.cfg.blocks.len() || stop.contains(&bi) {
                break;
            }
            let transfer = self.active_loops.last().and_then(|(header, exits)| {
                if bi == *header {
                    Some(Stmt::Continue)
                } else if exits.contains(&bi) {
                    Some(Stmt::Break)
                } else {
                    None
                }
            });
            if let Some(transfer) = transfer {
                // 回边或出口之前仍留在累加器中的调用必须在本轮执行。
                let saved = std::mem::take(&mut self.stmts);
                self.persist_value();
                out.extend(std::mem::replace(&mut self.stmts, saved));
                out.push(transfer);
                break;
            }
            if visited[bi] {
                break;
            }
            // 同一起点先包住最长区间；内层 catch 仍属于外层保护范围。
            let region = self
                .exception_regions
                .iter()
                .enumerate()
                .filter(|(n, r)| {
                    !self.active_regions.contains(n)
                        && r.start_pc as usize == self.cfg.blocks[bi].start_pc
                        && r.catch_blocks.len() == 1
                        && r.catch_blocks[0].type_idx == 0
                        && r.catch_blocks[0].handler_pc >= r.start_pc + r.length
                })
                .max_by_key(|(_, r)| r.length)
                .map(|(n, _)| n);
            if let Some(region) = region {
                // 入口之前计算的表达式必须在原来的保护范围求值。
                let saved = std::mem::take(&mut self.stmts);
                self.persist_value();
                out.extend(std::mem::replace(&mut self.stmts, saved));
                let (statement, next) = self.emit_exception_region(region, bi, stop, visited);
                out.push(statement);
                if let Some(next) = next {
                    bi = next;
                    continue;
                }
                break;
            }
            // 循环头：条件块中的计算必须每轮执行，而非移到循环之前。
            if let Some(lp) = self.cfg.loops.get(&bi).cloned() {
                // 前驱留下的调用只执行一次，不能延迟到循环条件块中。
                let saved = std::mem::take(&mut self.stmts);
                self.persist_value();
                out.extend(std::mem::replace(&mut self.stmts, saved));
                visited[bi] = true;
                let mut header = Vec::new();
                self.emit_block_insns(bi, &mut header);
                let (target, mn, _) = self.tail_of(bi);
                let header_exits: Vec<_> = self.cfg.blocks[bi]
                    .succs
                    .iter()
                    .copied()
                    .filter(|s| !lp.body.contains(s))
                    .collect();
                let loop_exits = if !header_exits.is_empty() {
                    header_exits
                } else {
                    lp.exits.clone()
                };
                self.active_loops.push((bi, loop_exits.clone()));
                let loop_stmt = if lp.is_do_while && lp.body.len() == 1 {
                    let cond = if target == Some(bi) {
                        self.branch_cond(&mn)
                    } else {
                        invert_cond(self.branch_cond(&mn))
                    };
                    Stmt::DoWhile(header, cond)
                } else if !lp.is_do_while {
                    let entry = self.cfg.blocks[bi]
                        .succs
                        .iter()
                        .copied()
                        .find(|x| lp.body.contains(x) && *x != bi)
                        .unwrap_or(bi);
                    let cond = if target == Some(entry) {
                        self.branch_cond(&mn)
                    } else {
                        invert_cond(self.branch_cond(&mn))
                    };
                    self.acc_persisted = true;
                    header.push(Stmt::If(invert_cond(cond), vec![Stmt::Break], Vec::new()));
                    let mut body = self.emit_seq(entry, stop, visited);
                    if matches!(body.last(), Some(Stmt::Continue)) {
                        body.pop();
                    }
                    header.extend(body);
                    Stmt::While(Expr::Bool(true), header)
                } else {
                    // 多块 do-while：将唯一条件回边作为尾部处理。
                    let latch = lp
                        .back_edges
                        .iter()
                        .copied()
                        .find(|b| self.cfg.blocks[*b].ends_cond)
                        .unwrap_or(bi);
                    let mut stops = stop.to_vec();
                    stops.push(latch);
                    if let Some(entry) = self.cfg.blocks[bi].succs.first().copied() {
                        header.extend(self.emit_seq(entry, &stops, visited));
                    }
                    self.emit_block_insns(latch, &mut header);
                    visited[latch] = true;
                    let (target, mn, _) = self.tail_of(latch);
                    let cond = if target == Some(bi) {
                        self.branch_cond(&mn)
                    } else {
                        invert_cond(self.branch_cond(&mn))
                    };
                    Stmt::DoWhile(header, cond)
                };
                self.active_loops.pop();
                out.push(loop_stmt);
                if let Some(&exit) = loop_exits.first() {
                    bi = exit;
                    continue;
                }
                break;
            }
            visited[bi] = true;
            self.emit_block_insns(bi, &mut out);
            // 块尾控制流
            let (target, mn, is_cond) = self.tail_of(bi);
            if let Some(t) = target {
                if is_cond {
                    // 条件分支：then/else 双臂到汇合块
                    let f = bi + 1;
                    let t_bi = t;
                    let cond = self.branch_cond(&mn);
                    self.acc_persisted = true;
                    // 汇合块：两臂首个公共多前驱块（兜底：较大臂的下一块）
                    let merge = self.find_merge(t_bi, f);
                    // then 始终对应跳转边，条件已经包含跳转方向。
                    let (then_entry, else_entry) = (t_bi, f);
                    let mut branch_stop = stop.to_vec();
                    branch_stop.push(merge);
                    let saved_regs = self.regs.clone();
                    let saved_acc = self.acc.clone();
                    let saved_visited = visited.clone();
                    let mut then_stmts = self.emit_seq(then_entry, &branch_stop, visited);
                    let then_visited = visited.clone();
                    *visited = saved_visited;
                    let then_acc = self.acc.clone();
                    let then_persisted = self.acc_persisted;
                    self.regs = saved_regs.clone();
                    self.acc = saved_acc;
                    self.acc_persisted = true;
                    let mut else_stmts = if else_entry != merge {
                        self.emit_seq(else_entry, &branch_stop, visited)
                    } else {
                        Vec::new()
                    };
                    for (v, t) in visited.iter_mut().zip(then_visited) {
                        *v |= t;
                    }
                    let else_acc = self.acc.clone();
                    let else_persisted = self.acc_persisted;
                    self.regs = saved_regs;
                    let temp = self.next_temp;
                    self.next_temp += 1;
                    if merge < self.cfg.blocks.len() {
                        then_stmts.push(Stmt::Expr(Expr::Assign(
                            Box::new(Expr::Reg(temp)),
                            Box::new(then_acc),
                        )));
                        else_stmts.push(Stmt::Expr(Expr::Assign(
                            Box::new(Expr::Reg(temp)),
                            Box::new(else_acc),
                        )));
                    } else {
                        // 两臂在不同保护区出口结束时不存在普通汇合值，仍须提交尾部调用。
                        if !then_persisted && has_effect(&then_acc) {
                            then_stmts.push(Stmt::Expr(then_acc));
                        }
                        if !else_persisted && has_effect(&else_acc) {
                            else_stmts.push(Stmt::Expr(else_acc));
                        }
                    }
                    self.acc = Expr::Reg(temp);
                    self.acc_persisted = true;
                    out.push(Stmt::If(cond, then_stmts, else_stmts));
                    bi = merge;
                    continue;
                }
                // 无条件跳转：目标在前 → 已处理（回边），在后 → 继续到目标
                bi = t;
                continue;
            }
            // 无跳转尾：return/throw 结束区域，否则 fall-through
            if self.ends_in_return(bi) {
                break;
            }
            bi += 1;
        }
        out
    }

    /// 按异常表构建嵌套保护区，独立发射正常路径和异常入口。
    fn emit_exception_region(
        &mut self,
        index: usize,
        start: usize,
        stop: &[usize],
        visited: &mut Vec<bool>,
    ) -> (Stmt, Option<usize>) {
        let region = self.exception_regions[index].clone();
        let catch = region.catch_blocks[0];
        let Some(&handler) = self.cfg.pc_to_block.get(&(catch.handler_pc as usize)) else {
            self.active_regions.push(index);
            let body = self.emit_seq(start, stop, visited);
            self.active_regions.pop();
            return (Stmt::Try(body, Vec::new()), None);
        };
        let end_pc = (region.start_pc + region.length) as usize;
        let finally = finally_cleanup(self.ins, catch.handler_pc as usize).filter(
            |(start, end, flag, continuation)| {
                // 清理期间的寄存器输出若在正常后继仍然活跃，局部规约可能误删用户赋值。
                // 未建立跨区域 SSA 时宁可保留展开形式。
                let scratch_only = self
                    .ins
                    .iter()
                    .filter(|i| i.pc >= *start && i.pc < *end)
                    .filter_map(written_register)
                    .all(|r| !register_live_from(self.ins, self.cfg, *continuation, r));
                let definition = self.ins.iter().enumerate().rev().find(|(_, i)| {
                    i.pc < region.start_pc as usize && written_register(i) == Some(*flag)
                });
                let initialized = definition.is_some_and(|(i, instruction)| {
                    i > 0
                        && instruction.meta.mnemonic == "sta"
                        && self.ins[i - 1].meta.mnemonic == "ldundefined"
                });
                let unchanged_flag = self
                    .ins
                    .iter()
                    .filter(|i| {
                        i.pc >= region.start_pc as usize
                            && i.pc < (region.start_pc + region.length) as usize
                    })
                    .all(|i| written_register(i) != Some(*flag));
                scratch_only && initialized && unchanged_flag
            },
        );
        let mut reachable = std::collections::HashSet::new();
        let mut pending = vec![start];
        while let Some(block) = pending.pop() {
            if self.cfg.blocks[block].start_pc < region.start_pc as usize
                || self.cfg.blocks[block].start_pc >= end_pc
                || !reachable.insert(block)
            {
                continue;
            }
            pending.extend(self.cfg.blocks[block].succs.iter().copied());
            for (n, inner) in self.exception_regions.iter().enumerate() {
                if n != index
                    && inner.start_pc >= region.start_pc
                    && inner.start_pc + inner.length <= region.start_pc + region.length
                    && self.cfg.blocks[block].start_pc >= inner.start_pc as usize
                    && self.cfg.blocks[block].start_pc < (inner.start_pc + inner.length) as usize
                {
                    for c in &inner.catch_blocks {
                        if let Some(h) = self.cfg.pc_to_block.get(&(c.handler_pc as usize)) {
                            pending.push(*h);
                        }
                    }
                }
            }
        }
        let mut exits = Vec::new();
        for block in reachable.iter().map(|b| &self.cfg.blocks[*b]) {
            for &successor in &block.succs {
                let pc = self.cfg.blocks[successor].start_pc;
                if successor == handler && finally.is_none() {
                    self.raw_fallback = Some("unverified normal exception-handler entry");
                }
                if (pc < region.start_pc as usize || pc >= end_pc)
                    && successor != handler
                    && !exits.contains(&successor)
                {
                    exits.push(successor);
                }
            }
        }
        // try 的尾边界常落在编译器插入的 jmp 上，而另一个分支直接跳到
        // 同一后继。只穿过无效果的控制胶水；不能把后继调用移入保护范围。
        exits = exits
            .into_iter()
            .map(|exit| self.skip_empty_jumps(exit, handler, stop))
            .filter(|exit| {
                // 循环的 continue/break 由 emit_seq 的活动循环上下文发射，
                // 不是异常区域之后需要再次发射的普通后继。
                !reachable.contains(exit)
                    && !self
                        .active_loops
                        .last()
                        .is_some_and(|(header, loop_exits)| {
                            exit == header || loop_exits.contains(exit)
                        })
            })
            .collect();
        exits.sort_unstable();
        exits.dedup();
        if finally.is_none() && exits.contains(&handler) {
            self.raw_fallback = Some("unverified normal exception-handler entry");
        }
        if finally.is_none() && exits.len() > 1 {
            // 多个出口若只经寄存器拷贝/常量装载到公共后继，可在 try 尾部
            // 保留这些不会抛异常的语句。真正可能抛异常的后继仍在 try 外。
            let join = exits.iter().copied().reduce(|a, b| self.find_merge(a, b));
            if let Some(join) = join.filter(|join| *join < self.cfg.blocks.len())
                && exits
                    .iter()
                    .all(|exit| self.reaches_join_without_throwing(*exit, join))
            {
                exits = vec![join];
            }
        }
        let mut stops = stop.to_vec();
        stops.push(handler);
        stops.extend(exits.iter().copied());
        let continuation_acc = (finally.is_none() && exits.len() == 1).then(|| {
            let temp = self.next_temp;
            self.next_temp += 1;
            temp
        });
        let saved_regs = self.regs.clone();
        self.active_regions.push(index);
        let mut body = self.emit_seq(start, &stops, visited);
        if let Some(temp) = continuation_acc {
            body.push(Stmt::Expr(Expr::Assign(
                Box::new(Expr::Reg(temp)),
                Box::new(self.peek_acc()),
            )));
            self.acc_persisted = true;
        } else if !self.acc_persisted && has_effect(&self.acc) {
            body.push(Stmt::Expr(self.take_acc()));
        }
        self.active_regions.pop();
        self.regs = saved_regs;
        let error = format!("error{index}");
        self.acc = Expr::Name(error.clone());
        self.acc_persisted = true;
        let mut catch_stops = stop.to_vec();
        catch_stops.extend(exits.iter().copied());
        let mut catch_body = self.emit_seq(handler, &catch_stops, visited);
        if let Some(temp) = continuation_acc {
            catch_body.push(Stmt::Expr(Expr::Assign(
                Box::new(Expr::Reg(temp)),
                Box::new(self.peek_acc()),
            )));
            self.acc_persisted = true;
        } else if !self.acc_persisted && has_effect(&self.acc) {
            catch_body.push(Stmt::Expr(self.take_acc()));
        }
        if let Some((start_pc, end_pc, flag, continuation)) = finally
            && let (Some(&cleanup_start), Some(&cleanup_end)) = (
                self.cfg.pc_to_block.get(&start_pc),
                self.cfg.pc_to_block.get(&end_pc),
            )
        {
            let saved_acc = self.acc.clone();
            let saved_persisted = self.acc_persisted;
            let saved_regs = self.regs.clone();
            let saved_loops = std::mem::take(&mut self.active_loops);
            self.acc = Expr::Bool(true);
            self.acc_persisted = true;
            let mut cleanup = self.emit_seq(
                cleanup_start,
                &[cleanup_end],
                &mut vec![false; self.cfg.blocks.len()],
            );
            if !self.acc_persisted && has_effect(&self.acc) {
                cleanup.push(Stmt::Expr(self.take_acc()));
            }
            if self.ins[self.cfg.blocks[cleanup_start].start].meta.mnemonic == "sta"
                && matches!(cleanup.first(), Some(Stmt::Expr(Expr::Assign(t, _))) if matches!(t.as_ref(), Expr::Reg(n) if *n == flag))
            {
                cleanup.remove(0);
            }
            self.acc = saved_acc;
            self.acc_persisted = saved_persisted;
            self.regs = saved_regs;
            self.active_loops = saved_loops;
            crate::decompile::structure::polish(&mut cleanup);
            // 此入口只可能携带真实异常，绝不等于内部 hole 哨兵。
            // 直接保留清理与重抛，避免把 hole 当成 JS undefined 丢失拒绝路径。
            catch_body = cleanup.clone();
            catch_body.push(Stmt::Throw(Expr::Name(error.clone())));
            let shared = !exits.is_empty()
                && exits.iter().all(|e| {
                    let pc = self.cfg.blocks[*e].start_pc;
                    pc == continuation || pc == (region.start_pc + region.length) as usize
                });
            self.finally_candidates
                .push((error.clone(), cleanup, flag, shared));
        }
        let mut statement = Stmt::Try(body, vec![(Some(error), catch_body)]);
        let shared_continuation = if let Some((_, _, _, continuation)) = finally
            && !exits.is_empty()
            && exits.iter().all(|e| {
                let pc = self.cfg.blocks[*e].start_pc;
                pc == continuation || pc == (region.start_pc + region.length) as usize
            }) {
            self.cfg.pc_to_block.get(&continuation).copied()
        } else {
            None
        };
        let next = if let Some(continuation) = shared_continuation {
            let mut trial = vec![statement.clone()];
            crate::decompile::structure::recover_finally(&mut trial, &self.finally_candidates);
            if matches!(trial.as_slice(), [Stmt::TryFinally(_, _)]) {
                statement = trial.remove(0);
                Some(continuation)
            } else {
                self.raw_fallback = Some("unverified shared finally continuation");
                None
            }
        } else if exits.len() == 1 {
            Some(exits[0])
        } else {
            if exits.len() > 1
                && !exits
                    .iter()
                    .all(|e| self.reaches_stop_without_effects(*e, stop))
            {
                self.raw_fallback = Some("multiple unresolved exception-region exits");
            }
            None
        };
        if let Some(temp) = continuation_acc {
            self.acc = Expr::Reg(temp);
            self.acc_persisted = true;
        }
        (statement, next)
    }

    /// debug 编译在保护区尾和外层边界之间插入只含跳转的独立块。
    fn skip_empty_jumps(&self, mut block: usize, handler: usize, stop: &[usize]) -> usize {
        let mut visited = std::collections::HashSet::new();
        while block != handler && !stop.contains(&block) && visited.insert(block) {
            let b = &self.cfg.blocks[block];
            if b.succs.len() != 1
                || self.ins[b.start..b.end]
                    .iter()
                    .any(|i| !matches!(i.meta.mnemonic, "nop" | "jmp"))
                || self
                    .exception_regions
                    .iter()
                    .any(|region| region.start_pc as usize == b.start_pc && region.length > 0)
            {
                break;
            }
            block = b.succs[0];
        }
        block
    }

    /// 仅允许不会抛异常的直线出口胶水进入保护区；禁止绕过另一异常区域。
    fn reaches_join_without_throwing(&self, mut block: usize, join: usize) -> bool {
        let mut visited = std::collections::HashSet::new();
        while block != join {
            if !visited.insert(block) {
                return false;
            }
            let b = &self.cfg.blocks[block];
            if b.succs.len() != 1
                || self.ins[b.start..b.end].iter().any(|i| {
                    !matches!(
                        i.meta.mnemonic,
                        "nop"
                            | "jmp"
                            | "mov"
                            | "lda"
                            | "sta"
                            | "ldundefined"
                            | "ldnull"
                            | "ldtrue"
                            | "ldfalse"
                            | "ldai"
                    )
                })
                || self
                    .exception_regions
                    .iter()
                    .any(|region| region.start_pc as usize == b.start_pc && region.length > 0)
            {
                return false;
            }
            block = b.succs[0];
        }
        true
    }

    /// debug 编译在保护区尾和外层边界之间插入只含跳转的独立块。
    fn reaches_stop_without_effects(&self, mut block: usize, stop: &[usize]) -> bool {
        let mut visited = std::collections::HashSet::new();
        loop {
            if stop.contains(&block) {
                return true;
            }
            if !visited.insert(block) {
                return false;
            }
            let b = &self.cfg.blocks[block];
            if b.succs.len() != 1
                || self.ins[b.start..b.end]
                    .iter()
                    .any(|i| !matches!(i.meta.mnemonic, "nop" | "jmp"))
            {
                return false;
            }
            block = b.succs[0];
        }
    }

    /// 块内指令提升（不含控制流处理）
    fn emit_block_insns(&mut self, bi: usize, out: &mut Vec<Stmt>) {
        let saved = std::mem::take(&mut self.stmts);
        let (start, end) = {
            let b = &self.cfg.blocks[bi];
            (b.start, b.end)
        };
        for k in start..end {
            self.visit(&self.ins[k]);
        }
        // 条件既用于选择边，也可能作为累加器值流入后继；必须只求值一次。
        if self.cfg.blocks[bi].ends_cond {
            self.persist_value();
        }
        out.extend(std::mem::replace(&mut self.stmts, saved));
    }

    /// 块尾信息：(目标块, 助记符, 是否条件跳转)
    fn tail_of(&self, bi: usize) -> (Option<usize>, String, bool) {
        let end = self.cfg.blocks[bi].end;
        let Some(last) = self.ins.get(end.saturating_sub(1)) else {
            return (None, String::new(), false);
        };
        let mn = last.mnemonic();
        if !last.jump_target().is_some() {
            return (None, mn, false);
        }
        let t = last
            .jump_target()
            .and_then(|t| self.cfg.pc_to_block.get(&t).copied());
        let is_cond = !mn.starts_with("jmp");
        (t, mn, is_cond)
    }

    /// 块是否以 return/throw 结尾
    fn ends_in_return(&self, bi: usize) -> bool {
        let end = self.cfg.blocks[bi].end;
        self.ins
            .get(end.saturating_sub(1))
            .map(|x| {
                let m = x.mnemonic();
                matches!(m.as_str(), "return" | "returnundefined" | "throw")
            })
            .unwrap_or(false)
    }

    /// 两臂汇合块：从较大下标臂向后找第一个多前驱块
    fn find_merge(&self, a: usize, b: usize) -> usize {
        let n = self.cfg.blocks.len();
        self.cfg.postdoms[a.min(n)]
            .iter()
            .copied()
            .filter(|p| self.cfg.postdoms[b.min(n)].contains(p))
            .max_by_key(|p| self.cfg.postdoms[*p].len())
            .unwrap_or(n)
    }

    fn branch_cond(&self, mnemonic: &str) -> Expr {
        if matches!(mnemonic, "jeqz" | "jstricteqz" | "jeqz.nan") {
            invert_cond(self.peek_acc())
        } else {
            self.peek_acc()
        }
    }

    fn visit(&mut self, i: &Ins) {
        let mn = i.mnemonic();
        // 宽指令只扩大操作数编码；共享完整语义分发，避免新增指令漏掉宽形式。
        let mn = mn.strip_prefix("wide.").unwrap_or(&mn);
        match mn {
            "nop" => {}
            "asyncfunctionawaituncaught" if self.is_async => {
                let value = self.take_acc();
                self.acc = Expr::Await(Box::new(value));
                self.acc_persisted = false;
            }
            // ---- 常量与寄存器 ----
            "ldundefined" => self.set_acc(Expr::Special("undefined")),
            "ldnull" => self.set_acc(Expr::Special("null")),
            "ldtrue" => self.set_acc(Expr::Bool(true)),
            "ldfalse" => self.set_acc(Expr::Bool(false)),
            "ldnan" => self.set_acc(Expr::Special("NaN")),
            "ldinfinity" => self.set_acc(Expr::Special("Infinity")),
            "ldthis" => self.set_acc(Expr::Special("this")),
            "ldnewtarget" => self.set_acc(Expr::Special("new.target")),
            "ldfunction" => self.set_acc(Expr::Special("arguments")),
            "ldhole" => {
                let temp = if let Some(temp) = self.hole_sentinel {
                    temp
                } else {
                    let temp = self.next_temp;
                    self.next_temp += 1;
                    self.hole_sentinel = Some(temp);
                    temp
                };
                self.set_acc(Expr::Reg(temp));
            }
            "ldsymbol" => self.set_acc(Expr::Special("Symbol")),
            "ldglobal" => self.set_acc(Expr::Special("globalThis")),
            "lda v" => unreachable!(),
            "lda" => {
                let v = self.reg_of(i);
                self.set_acc(self.v(v));
            }
            "sta" => {
                let v = self.reg_of(i);
                let e = self.peek_acc();
                self.set_v(v, e.clone());
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(self.v_target(v)),
                    Box::new(e),
                )));
                self.acc_persisted = true;
                self.acc = self.v_target(v);
            }
            "ldai" => {
                let v = signed_imm_of(i);
                self.set_acc(Expr::Int(v));
            }
            "fldai" | "f64ai" | "wide.ldai.64" => {
                let v = i.field("imm").unwrap_or(0);
                self.set_acc(Expr::Float(f64::from_bits(v)));
            }
            "lda.str" => {
                self.set_acc(Expr::Str(self.resolve_str(i)));
            }
            "ldai.64" => {
                let v = i.field("imm").unwrap_or(0);
                self.set_acc(Expr::Int(v as i64));
            }
            "mov" | "mov.v16" => {
                // mov v1, v2 = v[v1] := v[v2]（sig 首操作数恒为 dst）。
                // 寄存器搬家必须落为语句：es2abc 用 mov 三连做 a/b 交换
                // （mov v3,v0; mov v0,v1; mov v1,v3），丢弃即丢失循环携带依赖
                let vs = i.sig_values();
                let dst = vs
                    .iter()
                    .find(|(n, _, _)| n.starts_with('v'))
                    .map(|(_, _, v)| *v)
                    .unwrap_or(0);
                let src = vs
                    .iter()
                    .filter(|(n, _, _)| n.starts_with('v'))
                    .nth(1)
                    .map(|(_, _, v)| *v)
                    .unwrap_or(dst);
                self.snapshot_acc_before_register_write(dst);
                let src_e = self.v(src);
                self.set_v(dst, src_e.clone());
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(self.v_target(dst)),
                    Box::new(src_e),
                )));
            }
            // ---- 二元运算 ----
            "add2" => self.bin("+", i),
            "sub2" => self.bin("-", i),
            "mul2" => self.bin("*", i),
            "div2" => self.bin("/", i),
            "mod2" => self.bin("%", i),
            "exp2" => self.bin("**", i),
            "shl2" => self.bin("<<", i),
            "shr2" => self.bin(">>>", i),
            "ashr2" => self.bin(">>", i),
            "and2" => self.bin("&", i),
            "or2" => self.bin("|", i),
            "xor2" => self.bin("^", i),
            "eq" => self.bin("==", i),
            "neq" | "noteq" => self.bin("!=", i),
            "stricteq2" => self.bin("===", i),
            "strictneq2" => self.bin("!==", i),
            "exp" => self.bin("**", i),
            "neg" => {
                // acc = -v（无 v 时取反 acc）
                if i.sig_values().iter().any(|(n, _, _)| n.starts_with('v')) {
                    let v = self.reg_of(i);
                    let e = self.v(v);
                    self.set_acc(Expr::Un("-", Box::new(e)));
                } else {
                    let e = self.take_acc();
                    self.set_acc(Expr::Un("-", Box::new(e)));
                }
            }
            "inc" => {
                // acc = acc + 1（es2abc 的 ++ 降级为 tonumeric + inc）
                let e = self.take_acc();
                self.set_acc(Expr::Bin("+", Box::new(e), Box::new(Expr::Int(1))));
            }
            "dec" => {
                let e = self.take_acc();
                self.set_acc(Expr::Bin("-", Box::new(e), Box::new(Expr::Int(1))));
            }
            "tonumeric" => {
                let e = self.take_acc();
                self.set_acc(Expr::Un("+", Box::new(e)));
            }
            "definefieldbyname" | "definepropertybyname" => {
                // DefineField 使用 CreateDataPropertyOrThrow；不能改写为会触发继承 setter 的赋值。
                // 两个指令都保留 acc，imm 只是内联缓存槽位。
                let v = self.reg_of(i);
                let name = self.resolve_str(i);
                self.define_field(self.v(v), Expr::Str(name));
            }
            "stownbyvaluewithnameset" => {
                let regs = self.regs_of(i);
                self.store_own_with_name(self.v(regs[0]), self.v(regs[1]));
            }
            "stownbynamewithnameset" => {
                self.store_own_with_name(self.v(self.reg_of(i)), Expr::Str(self.resolve_str(i)));
            }
            "stownbyname" => {
                // 对象.K = acc
                let v = self.reg_of(i);
                let name = self.resolve_str(i);
                let val = self.persist_value();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Member(Box::new(self.v(v)), name)),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "stricteq" => self.bin("===", i),
            "strictnoteq" => self.bin("!==", i),
            "less" => self.bin("<", i),
            "lesseq" => self.bin("<=", i),
            "greater" => self.bin(">", i),
            "greatereq" => self.bin(">=", i),
            "instanceof" => self.bin("instanceof", i),
            "in" | "isin" => self.bin("in", i),
            "istrue" => {
                let e = self.take_acc();
                self.set_acc(Expr::Un("!", Box::new(Expr::Un("!", Box::new(e)))));
            }
            "isfalse" => {
                let e = self.take_acc();
                self.set_acc(invert_cond(e));
            }
            // ---- 一元 ----
            "negate" => self.un("-", i),
            "not" => self.un("~", i),
            "tonumber" => self.un("Number", i),
            "typeof" => self.un("typeof", i),
            "toboolean" => self.un("Boolean", i),
            "incr2" => self.update("++", i, true),
            "decr2" => self.update("--", i, true),
            "incr" => {
                let v = self.reg_of(i);
                self.set_acc(Expr::Update("++", Box::new(self.v(v)), true));
            }
            "decr" => {
                let v = self.reg_of(i);
                self.set_acc(Expr::Update("--", Box::new(self.v(v)), true));
            }
            "neg-inf" | "exp-by" | "log2e" | "ln2" | "infinity" => self.fallback(i),
            // ---- 属性 ----
            "ldthisbyname" => {
                // acc = this.<name>（0.1.0.x 覆盖表）
                let name = self.resolve_str(i);
                if name.is_empty() {
                    let id = i
                        .sig_values()
                        .iter()
                        .find(|(n, _, _)| n.ends_with("string_id"))
                        .map(|(_, _, v)| *v)
                        .unwrap_or(0);
                    self.set_acc(Expr::Member(
                        Box::new(Expr::Special("this")),
                        format!("/*id {id:#x}*/"),
                    ));
                } else {
                    self.set_acc(Expr::Member(Box::new(Expr::Special("this")), name));
                }
            }
            "stthisbyname" => {
                // this.<name> = acc
                let mut name = self.resolve_str(i);
                if name.is_empty() {
                    let id = i
                        .sig_values()
                        .iter()
                        .find(|(n, _, _)| n.ends_with("string_id"))
                        .map(|(_, _, v)| *v)
                        .unwrap_or(0);
                    name = format!("prop_{id:x}");
                }
                let val = self.persist_value();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Member(Box::new(Expr::Special("this")), name)),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "ldobjbyname" => {
                // acc = acc.<name>（无寄存器操作数）
                let name = self.resolve_str(i);
                let obj = self.take_acc();
                self.set_acc(Expr::Member(Box::new(obj), name));
            }
            "stobjbyname" => {
                let v = self.reg_of(i);
                let name = self.resolve_str(i);
                let val = self.persist_value();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Member(Box::new(self.v(v)), name)),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "ldobjbyvalue" => {
                // acc = v[acc]
                let v = self.reg_of(i);
                let key = self.take_acc();
                self.set_acc(Expr::Index(Box::new(self.v(v)), Box::new(key)));
            }
            "ldprivateproperty" | "stprivateproperty" => {
                self.private_property(i, mn == "stprivateproperty", false)
            }
            "stobjbyvalue" | "stownbyvalue" => {
                // v1[v2] = acc
                let vs = self.regs_of(i);
                let obj = vs.first().copied().unwrap_or(0);
                let key = vs.get(1).copied().unwrap_or(0);
                let val = self.persist_value();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Index(Box::new(self.v(obj)), Box::new(self.v(key)))),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "ldobjbyindex" => {
                let obj = self.take_acc();
                let idx = self.imm_last(i);
                self.set_acc(Expr::Index(Box::new(obj), Box::new(Expr::Int(idx as i64))));
            }
            "stobjbyindex" => {
                let v = self.reg_of(i);
                let idx = self.imm_last(i);
                let val = self.persist_value();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Index(
                        Box::new(self.v(v)),
                        Box::new(Expr::Int(idx as i64)),
                    )),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "ldstaticarray" | "ststaticarray" | "ldarray" | "starray" => self.fallback(i),
            // ---- 调用 ----
            "callarg0" | "callarg1" | "callarg2" | "callarg3" | "callarg4" | "callargs2"
            | "callargs3" => {
                // 函数在 acc；sig 中显式 v 字段即参数
                let callee = self.take_acc();
                let args: Vec<Expr> = i
                    .sig_values()
                    .iter()
                    .filter(|(n, _, _)| n.starts_with('v'))
                    .map(|(_, _, v)| self.v(*v))
                    .collect();
                self.set_acc(call_expr(callee, args));
            }
            "callthis0" | "callthis1" | "callthis2" | "callthis3" | "callthis4" | "callthis5"
            | "callthis6" | "callthis7" => {
                // 函数在 acc；第一个 v 是 this，其余是参数
                let vs: Vec<u64> = i
                    .sig_values()
                    .iter()
                    .filter(|(n, _, _)| n.starts_with('v'))
                    .map(|(_, _, v)| *v)
                    .collect();
                let this_arg = vs.first().map(|&v| Box::new(self.v(v)));
                let args: Vec<Expr> = vs.iter().skip(1).map(|&v| self.v(v)).collect();
                let callee = self.take_acc();
                self.set_acc(Expr::Call {
                    callee: Box::new(callee),
                    args,
                    new: false,
                    this_arg,
                });
            }
            "callrange" | "callthisrange" | "newobjrange" => {
                // Panda ISA：v[start] = 函数（或构造器）；callthisrange 的
                // v[start+1] = this；其余为参数。acc 不参与
                let count = self.imm_last(i) as usize;
                let start = self.reg_of(i) as usize;
                let is_new = mn == "newobjrange";
                let callee = if is_new {
                    self.v(start as u64)
                } else {
                    self.take_acc()
                };
                let (this_arg, arg_start, arg_count) = if mn == "callthisrange" {
                    (Some(Box::new(self.v(start as u64))), start + 1, count)
                } else {
                    if is_new {
                        (None, start + 1, count.saturating_sub(1))
                    } else {
                        (None, start, count)
                    }
                };
                let args: Vec<Expr> = (0..arg_count)
                    .map(|k| self.v((arg_start + k) as u64))
                    .collect();
                self.set_acc(Expr::Call {
                    callee: Box::new(callee),
                    args,
                    new: is_new,
                    this_arg,
                });
            }
            "supercallthisrange" | "supercallarrowrange" => {
                let start = self.reg_of(i);
                let count = self.imm_last(i);
                self.set_acc(call_expr(
                    Expr::Special("super"),
                    (0..count).map(|n| self.v(start + n)).collect(),
                ));
            }
            "supercallspread" => {
                // acc 是当前函数对象，v 是实参数组；结果为 super 构造的接收者。
                self.set_acc(call_expr(
                    Expr::Special("super"),
                    vec![Expr::Spread(Box::new(self.v(self.reg_of(i))))],
                ));
            }
            "ldsuperbyname" => {
                if !self.super_receivers.contains(&i.pc) {
                    self.fallback(i);
                    return;
                }
                let name = self.resolve_str(i);
                self.set_acc(Expr::Member(Box::new(Expr::Special("super")), name));
            }
            "ldsuperbyvalue" => {
                if !self.super_receivers.contains(&i.pc) {
                    self.fallback(i);
                    return;
                }
                let key = self.take_acc();
                self.set_acc(Expr::Index(Box::new(Expr::Special("super")), Box::new(key)));
            }
            "stsuperbyname" | "stsuperbyvalue" => {
                if !self.super_receivers.contains(&i.pc) {
                    self.fallback(i);
                    return;
                }
                let target = if mn == "stsuperbyname" {
                    Expr::Member(Box::new(Expr::Special("super")), self.resolve_str(i))
                } else {
                    Expr::Index(
                        Box::new(Expr::Special("super")),
                        Box::new(self.v(self.regs_of(i)[1])),
                    )
                };
                let value = self.persist_value();
                self.stmts
                    .push(Stmt::Expr(Expr::Assign(Box::new(target), Box::new(value))));
                self.acc_persisted = true;
            }
            "newobjapply" => {
                // v 是构造器，acc 是数组；运行时按下标读取，不调用数组迭代器。
                let args = self.take_acc();
                self.set_acc(call_expr(
                    Expr::TemplateLit("((constructor, args) => { if (!globalThis.Array.isArray(args)) throw new globalThis.TypeError(\"Cannot Newobjspread\"); return globalThis.Reflect.construct(constructor, args); })".into()),
                    vec![self.v(self.reg_of(i)), args],
                ));
            }
            "callspread" | "callthisspread" => {
                // v0=函数 v1=this v2=参数数组（spread）
                let callee = self.v(0);
                let this_arg = if mn == "callthisspread" {
                    Some(Box::new(self.v(1)))
                } else {
                    None
                };
                let args = vec![Expr::Special("...args")];
                self.set_acc(Expr::Call {
                    callee: Box::new(callee),
                    args,
                    new: false,
                    this_arg,
                });
            }
            "defineclasswithbuffer" | "definesendableclass" => {
                // v 是父类输入；类对象只写入累加器。
                let off = self.litarr_of(i).unwrap_or(0);
                self.set_acc(Expr::ClassLit(off));
            }
            // ---- 函数定义 / 字面量 ----
            "definefunc"
            | "definemethod"
            | "definefuncwithname"
            | "definegeneratorfunc"
            | "defineasyncfunc"
            | "defineasynccGeneratorfunc" => {
                let off = self.resolve_method_name(i).map(|(o, _)| o).unwrap_or(0);
                self.set_acc(Expr::Closure { method_off: off });
            }
            "createobjectwithbuffer" => {
                let off = self.litarr_of(i).unwrap_or(0);
                self.set_acc(Expr::ObjectLit(off));
            }
            "createarraywithbuffer" => {
                let off = self.litarr_of(i).unwrap_or(0);
                self.set_acc(Expr::ArrayLit(off));
            }
            "createemptyobject" => self.set_acc(Expr::ObjectLit(0)),
            // ---- 词法环境 ----
            "ldlexvar" | "deprecated.ldlexvar" | "ldlexvar2" | "wide.ldlexvar" => {
                // acc = lexenv[imm1][imm2]，无寄存器操作数
                let (depth, slot) = two_imms(i);
                let binding = self
                    .lexical_names
                    .get(&i.pc)
                    .cloned()
                    .map(Expr::Name)
                    .unwrap_or(Expr::LexVar(depth as u32, slot as u32));
                self.set_acc(binding);
            }

            "stlexvar" | "deprecated.stlexvar" => {
                let (depth, slot) = two_imms(i);
                let v = self.reg_of(i);
                let val = self.persist_value();
                let binding = self
                    .lexical_names
                    .get(&i.pc)
                    .cloned()
                    .map(Expr::Name)
                    .unwrap_or(Expr::LexVar(depth as u32, slot as u32));
                self.stmts
                    .push(Stmt::Expr(Expr::Assign(Box::new(binding), Box::new(val))));
                let _ = v;
                self.acc_persisted = true;
            }
            "newlexenv" | "deprecated.newlexenv" | "newlexenvwithname" => {
                // withname 变体：literal array 里是槽位名表
                let names = if mn.contains("withname") {
                    self.litarr_of(i).and_then(|off| {
                        self.file.read_literal_array_truncated(off).ok().map(|t| {
                            t.values
                                .iter()
                                .filter_map(|v| match v {
                                    crate::file::LiteralValue::String(o) => {
                                        self.file.string_at(*o).ok()
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                        })
                    })
                } else {
                    None
                };
                self.lex_names.push(names);
            }
            "poplexenv" => {
                self.lex_names.pop();
            }
            "ldlocalmodulevar" => {
                // acc = 本 record 的本地模块导出变量
                let idx = self.imm(i);
                self.set_acc(Expr::Name(self.module_local_name(idx)));
            }
            "stmodulevar" => {
                // 模块导出赋值：export_name = acc
                let idx = self.imm(i);
                let val = self.persist_value();
                let name = self.module_export_name(idx);
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Name(format!("export:{name}"))),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "getmodulenamespace" => {
                let request = self.imm(i);
                let namespace = self
                    .module_namespace_name(request)
                    .map(|name| Expr::Name(name.to_owned()))
                    .unwrap_or(Expr::Special("import namespace"));
                self.set_acc(namespace);
            }
            "ldexternalmodulevar" => {
                // acc = 本 record 模块导入表中的绑定（regular/namespace import）
                let idx = self.imm(i);
                self.set_acc(Expr::Name(self.module_import_name(idx)));
            }
            "dynamicimport" | "deprecated.dynamicimport" => {
                let specifier = self.take_acc();
                self.set_acc(call_expr(Expr::Name("import".into()), vec![specifier]));
            }
            "delobjprop" => {
                // acc 是属性键，v 是目标对象；结果为 delete 表达式的布尔值。
                let key = self.take_acc();
                let object = self.v(self.reg_of(i));
                self.set_acc(Expr::Un(
                    "delete",
                    Box::new(Expr::Index(Box::new(object), Box::new(key))),
                ));
            }
            // ---- 变参 / apply / spread ----
            "getunmappedargs" => self.set_acc(Expr::Special("arguments")),
            "copyrestargs" | "wide.copyrestargs" => {
                // 剩余参数数组进 acc（无寄存器操作数），渲染为 args / ...args
                self.set_acc(call_expr(
                    Expr::Name("Array.prototype.slice.call".into()),
                    vec![
                        Expr::Special("arguments"),
                        Expr::Int(self.imm_last(i) as i64),
                    ],
                ));
            }
            "createemptyarray" | "createarray" => {
                // 数组进 acc（无寄存器操作数），构建器在 stownbyindex 惰性建立
                self.set_acc(Expr::Special("[]"));
            }
            "stownbyindex" => {
                let arr = self.reg_of(i);
                let idx = self.imm_last(i);
                let val = self.persist_value();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Index(
                        Box::new(self.v(arr)),
                        Box::new(Expr::Int(idx as i64)),
                    )),
                    Box::new(val),
                )));
            }
            "starrayspread" => {
                // dst[v2..] = ...acc，返回末尾下标；不能调用可被覆盖的 push。
                let regs = self.regs_of(i);
                let arr = self.v(regs[0]);
                let index = self.v(regs[1]);
                let value = self.take_acc();
                let same =
                    |a: &Expr, b: &Expr| crate::decompile::printer::expr_eq_bounded(a, b, &mut 64);
                // 编译器的连续空数组/零下标初始化尚未逃逸，可直接恢复数组展开。
                let fresh = matches!(self.stmts.as_slice(), [..,
                    Stmt::Expr(Expr::Assign(a, empty)),
                    Stmt::Expr(Expr::Assign(idx, zero))]
                    if same(a, &arr) && same(idx, &index)
                        && matches!(arr, Expr::Reg(_))
                        && !same(&arr, &index) && !same(&arr, &value)
                        && matches!(empty.as_ref(), Expr::Special("[]"))
                        && matches!(zero.as_ref(), Expr::Int(0))
                        && matches!(value, Expr::Reg(_) | Expr::Arg(_) | Expr::Special("arguments")));
                if fresh {
                    self.stmts.remove(self.stmts.len() - 2);
                    self.stmts.push(Stmt::Expr(Expr::Assign(
                        Box::new(arr.clone()),
                        Box::new(Expr::Array(vec![Expr::Spread(Box::new(value))])),
                    )));
                    self.acc = Expr::Member(Box::new(arr), "length".into());
                    // 新数组的 length 是固有数据属性；丢弃返回下标不产生额外读取。
                    self.acc_persisted = true;
                } else {
                    let source = self.next_temp;
                    let target = source + 1;
                    let next = source + 2;
                    let empty = source + 3;
                    self.next_temp += 4;
                    for (reg, expr) in [
                        (source, value),
                        (target, arr),
                        (next, index),
                        (empty, Expr::Bool(true)),
                    ] {
                        self.stmts.push(Stmt::Expr(Expr::Assign(
                            Box::new(Expr::Reg(reg)),
                            Box::new(expr),
                        )));
                    }
                    let item = format!("spreadValue{source}");
                    self.stmts.push(Stmt::ForOf(
                        item.clone(),
                        Expr::Reg(source),
                        vec![
                            Stmt::Expr(call_expr(
                                Expr::Special("globalThis.Object.defineProperty"),
                                vec![
                                    Expr::Reg(target),
                                    Expr::Reg(next),
                                    Expr::Object(vec![
                                        ("value".into(), Expr::Name(item)),
                                        ("writable".into(), Expr::Bool(true)),
                                        ("enumerable".into(), Expr::Bool(true)),
                                        ("configurable".into(), Expr::Bool(true)),
                                    ]),
                                ],
                            )),
                            Stmt::Expr(Expr::Update("++", Box::new(Expr::Reg(next)), true)),
                            Stmt::Expr(Expr::Assign(
                                Box::new(Expr::Reg(empty)),
                                Box::new(Expr::Bool(false)),
                            )),
                        ],
                    ));
                    // 空输入仍将目标数组截到起始下标，不能把整个指令视作无操作。
                    self.stmts.push(Stmt::If(
                        Expr::Reg(empty),
                        vec![Stmt::Expr(Expr::Assign(
                            Box::new(Expr::Member(Box::new(Expr::Reg(target)), "length".into())),
                            Box::new(Expr::Reg(next)),
                        ))],
                        Vec::new(),
                    ));
                    self.acc = Expr::Reg(next);
                    self.acc_persisted = true;
                }
            }
            "copydataproperties" => self.copy_data_properties(self.v(self.reg_of(i))),
            "apply" => {
                let vs = self.regs_of(i);
                let callee = self.take_acc();
                self.set_acc(Expr::Call {
                    callee: Box::new(callee),
                    args: vec![Expr::Spread(Box::new(
                        self.v(vs.get(1).copied().unwrap_or(0)),
                    ))],
                    new: false,
                    this_arg: Some(Box::new(self.v(vs.first().copied().unwrap_or(0)))),
                });
            }
            "ldglobalvar" => {
                let v = self.reg_of(i);
                let name = self.resolve_str(i);
                self.set_v(v, Expr::Member(Box::new(Expr::Special("globalThis")), name));
                let e = self.v(v);
                self.stmts.push(Stmt::Expr(e));
            }
            // try 系全局加载：acc = globalThis.<name>（未声明全局的软引用）
            "tryldglobalbyname" | "tryldglobalbyvalue" | "tryldglobalbyindex" => {
                let key_expr = if mn == "tryldglobalbyname" {
                    None
                } else {
                    Some(self.take_acc())
                };
                match key_expr {
                    // acc 键为字符串常量：按名渲染
                    Some(Expr::Str(name)) => {
                        self.set_acc(Expr::Member(Box::new(Expr::Special("globalThis")), name))
                    }
                    // 动态键：索引形式
                    Some(e) => self.set_acc(Expr::Index(
                        Box::new(Expr::Special("globalThis")),
                        Box::new(e),
                    )),
                    None => {
                        let name = self.resolve_str(i);
                        self.set_acc(Expr::Member(Box::new(Expr::Special("globalThis")), name));
                    }
                }
            }
            "stglobalvar" | "stglobalrecord" => {
                let v = self.reg_of(i);
                let name = self.resolve_str(i);
                let val = self.persist_value();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Member(Box::new(Expr::Special("globalThis")), name)),
                    Box::new(val),
                )));
                let _ = v;
                self.acc_persisted = true;
            }
            "ldglobalrecord" => {
                let v = self.reg_of(i);
                let name = self.resolve_str(i);
                self.set_v(v, Expr::Name(name));
                let e = self.v(v);
                self.stmts.push(Stmt::Expr(e));
            }
            // ---- 控制流 ----
            "jmp" => {}
            "jeqz" | "jnez" | "jstricteqz" | "jnstricteqz" | "jeqz.nan" => {
                // 条件已在块尾处理；这里不动
            }
            "return" => {
                let e = self.take_acc();
                self.stmts.push(Stmt::Return(Some(e)));
            }
            "returnundefined" => {
                let persisted = self.acc_persisted;
                let dropped = self.take_acc();
                if !persisted && has_effect(&dropped) && !matches!(dropped, Expr::Special(_)) {
                    self.stmts.push(Stmt::Expr(dropped));
                }
                self.stmts.push(Stmt::Return(None));
            }
            "throw" => {
                let e = self.take_acc();
                self.stmts.push(Stmt::Throw(e));
            }
            "throw.ifnotobject" => {
                let value = self.v(self.reg_of(i));
                self.require_object(value);
            }
            // ---- 生成器/协程脚手架 ----
            "suspendgenerator"
            | "resumegenerator"
            | "getresumemode"
            | "createiterresultobj"
            | "gettemplateobject"
            | "setgeneratorstate"
            | "gettemplateobject.no_ic" => {
                self.fallback(i);
            }
            "getiterator" => {
                // Panda GETITERATOR 与 JS 抽象 GetIterator 不同：非可调用的
                // @@iterator 原样返回，结果对象检查由后续字节码明确执行。
                let recv = self.take_acc();
                let receiver = self.next_temp;
                let method = receiver + 1;
                self.next_temp += 2;
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Reg(receiver)),
                    Box::new(recv),
                )));
                let iterator_method = Expr::Index(
                    Box::new(Expr::Reg(receiver)),
                    Box::new(Expr::Member(
                        Box::new(Expr::Name("Symbol".into())),
                        "iterator".into(),
                    )),
                );
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Reg(method)),
                    Box::new(iterator_method),
                )));
                self.set_acc(Expr::Cond(
                    Box::new(Expr::Bin(
                        "===",
                        Box::new(Expr::Un("typeof", Box::new(Expr::Reg(method)))),
                        Box::new(Expr::Str("function".into())),
                    )),
                    Box::new(Expr::Call {
                        callee: Box::new(Expr::Member(
                            Box::new(Expr::Name("Reflect".into())),
                            "apply".into(),
                        )),
                        args: vec![Expr::Reg(method), Expr::Reg(receiver), Expr::Array(vec![])],
                        new: false,
                        this_arg: None,
                    }),
                    Box::new(Expr::Reg(method)),
                ));
            }
            "getasynciterator" => {
                let recv = self.take_acc();
                self.set_acc(Expr::Call {
                    callee: Box::new(Expr::Index(
                        Box::new(recv),
                        Box::new(Expr::Member(
                            Box::new(Expr::Name("Symbol".into())),
                            if mn == "getasynciterator" {
                                "asyncIterator"
                            } else {
                                "iterator"
                            }
                            .into(),
                        )),
                    )),
                    args: Vec::new(),
                    new: false,
                    this_arg: None,
                });
            }
            "getpropiterator" => {
                let recv = self.take_acc();
                // for-in 包含可枚举的继承属性，且迭代期间会检查属性删除。
                self.set_acc(call_expr(
                    Expr::TemplateLit(
                        "(function* (value) { for (const key in value) yield key; })".into(),
                    ),
                    vec![recv],
                ));
            }
            "getnextpropname" => {
                let next = call_expr(
                    Expr::Member(Box::new(self.v(self.reg_of(i))), "next".into()),
                    vec![],
                );
                self.set_acc(Expr::Member(Box::new(next), "value".into()));
            }
            // ---- 运行时调用 ----
            other => {
                if let Some(rest) = other.strip_prefix("callruntime.") {
                    self.lift_callruntime(rest, i);
                } else if other.starts_with("throw.") {
                    // 编译器生成的 TDZ/参数检查（throw.undefinedifholewithname 等）
                    // 在源码中不存在，跳过
                    if !other.contains("ifhole") && !other.contains("ifsupernotcorrectcall") {
                        let name = self.resolve_str(i);
                        let what = if name.is_empty() {
                            other.to_string()
                        } else {
                            format!("{other} {name:?}")
                        };
                        self.stmts.push(Stmt::Comment(what));
                    }
                } else {
                    self.fallback(i);
                }
            }
        }
    }

    fn lift_callruntime(&mut self, name: &str, i: &Ins) {
        match name {
            "ldlazymodulevar"
            | "wideldlazymodulevar"
            | "ldlazysendablemodulevar"
            | "wideldlazysendablemodulevar" => {
                let idx = self.imm(i);
                self.set_acc(Expr::Name(self.lazy_module_import_name(idx)));
            }
            "ldsendableexternalmodulevar" | "wideldsendableexternalmodulevar" => {
                self.set_acc(Expr::Name(self.module_import_name(self.imm(i))));
            }
            "ldsendablelocalmodulevar" | "wideldsendablelocalmodulevar" => {
                self.set_acc(Expr::Name(self.module_local_name(self.imm(i))));
            }
            "definesendableclass" => {
                self.set_acc(Expr::ClassLit(self.litarr_of(i).unwrap_or(0)));
            }
            "callinit" => {
                let callee = self.take_acc();
                self.set_acc(Expr::Call {
                    callee: Box::new(callee),
                    args: vec![],
                    new: false,
                    this_arg: Some(Box::new(self.v(self.reg_of(i)))),
                });
            }
            "createprivateproperty" => {
                // 起始槽必须由 record 级环境分析证明；不能把缓存槽当作私有键。
                let first = self
                    .lexical_names
                    .get(&i.pc)
                    .and_then(|name| name.rsplit_once("_slot"));
                if let Some((prefix, first)) = first
                    && let Ok(first) = first.parse::<u32>()
                {
                    let prefix = prefix.to_owned();
                    for slot in first..first + self.imm(i) as u32 {
                        self.stmts.push(Stmt::Expr(Expr::Assign(
                            Box::new(Expr::Name(format!("{prefix}_slot{slot}"))),
                            Box::new(Expr::Call {
                                callee: Box::new(Expr::Special("globalThis.WeakMap")),
                                args: vec![],
                                new: true,
                                this_arg: None,
                            }),
                        )));
                    }
                    self.set_acc(Expr::Special("undefined"));
                } else {
                    self.fallback(i);
                }
            }
            "defineprivateproperty" => self.private_property(i, true, true),
            "definefieldbyvalue" => {
                // v1 是键，v2 才是接收者；imm 是缓存槽，不是属性下标。
                let regs = self.regs_of(i);
                self.define_field(self.v(regs[1]), self.v(regs[0]));
            }
            "definefieldbyindex" => {
                self.define_field(self.v(self.reg_of(i)), Expr::Int(self.imm_last(i) as i64))
            }
            "topropertykey" => {
                let key = self.take_acc();
                self.set_acc(to_property_key(key));
            }
            "definefunc" | "definemethod" | "defineclasswithbuffer" => {
                let vs = i.sig_values();
                let out_reg = vs
                    .iter()
                    .find(|(n, _, _)| n.starts_with('v'))
                    .map(|(_, _, v)| *v);
                let off = self.resolve_method_name(i).map(|(o, _)| o).unwrap_or(0);
                if let Some(v) = out_reg {
                    self.set_v(v, Expr::Closure { method_off: off });
                    let e = self.v(v);
                    self.stmts.push(Stmt::Expr(Expr::Call {
                        callee: Box::new(Expr::Name(format!("/*{name}*/"))),
                        args: vec![e],
                        new: false,
                        this_arg: None,
                    }));
                }
            }
            "ldprivateproperty" | "stprivateproperty" | "calli0" | "calli1" | "newlexenv"
            | "defineclass" => {
                self.fallback(i);
            }
            "isfalse" => {
                let e = self.take_acc();
                self.set_acc(Expr::Un("!", Box::new(e)));
            }
            "istrue" => {
                // ToBoolean 强制转换：分支条件上下文，保留原表达式
            }
            _ => self.fallback(i),
        }
    }

    /// 数据私有字段的键来自词法环境；WeakMap 不暴露字段，并允许冻结对象初始化。
    fn private_property(&mut self, i: &Ins, store: bool, define: bool) {
        let Some(key) = self
            .lexical_names
            .get(&i.pc)
            .filter(|name| name.starts_with("private_"))
            .cloned()
        else {
            self.fallback(i);
            return;
        };
        let value = self.take_acc();
        let mut args = vec![
            if store {
                self.v(self.reg_of(i))
            } else {
                value.clone()
            },
            Expr::Name(key),
        ];
        let helper = if define {
            args.push(value);
            "((object, key, value) => { if (key.has(object)) throw new globalThis.TypeError(\"Private field already exists\"); key.set(object, value); return true; })"
        } else if store {
            args.push(value);
            "((object, key, value) => { if (!key.has(object)) throw new globalThis.TypeError(\"Invalid private field receiver\"); key.set(object, value); return true; })"
        } else {
            "((object, key) => { if (!key.has(object)) throw new globalThis.TypeError(\"Invalid private field receiver\"); return key.get(object); })"
        };
        self.set_acc(call_expr(Expr::TemplateLit(helper.into()), args));
    }

    fn bin(&mut self, op: &'static str, i: &Ins) {
        // es2abc 计算顺序：RHS 先入 acc，左操作数在 v 寄存器 → acc = v OP acc
        let v = self.reg_of(i);
        let rhs = self.take_acc();
        let lhs = self.v(v);
        // hole 可经过多个块和寄存器拷贝；运行时身份判别不依赖局部传播。
        // 只在实际比较 hole 时跳过 JS ToPrimitive，其余值保留原宽松相等语义。
        // RHS 先落地一次，避免判别和比较重复执行属性读取/调用。
        if let Some(hole) = self.hole_sentinel
            && matches!(op, "==" | "!=")
        {
            let rhs = if matches!(rhs, Expr::Reg(_) | Expr::Arg(_)) {
                rhs
            } else {
                let temp = self.next_temp;
                self.next_temp += 1;
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Reg(temp)),
                    Box::new(rhs),
                )));
                Expr::Reg(temp)
            };
            let hole_check = Expr::Bin(
                "||",
                Box::new(Expr::Bin(
                    "===",
                    Box::new(lhs.clone()),
                    Box::new(Expr::Reg(hole)),
                )),
                Box::new(Expr::Bin(
                    "===",
                    Box::new(rhs.clone()),
                    Box::new(Expr::Reg(hole)),
                )),
            );
            self.set_acc(Expr::Cond(
                Box::new(hole_check),
                Box::new(Expr::Bin(
                    if op == "==" { "===" } else { "!==" },
                    Box::new(lhs.clone()),
                    Box::new(rhs.clone()),
                )),
                Box::new(Expr::Bin(op, Box::new(lhs), Box::new(rhs))),
            ));
            return;
        }
        self.set_acc(Expr::Bin(op, Box::new(lhs), Box::new(rhs)));
    }

    fn un(&mut self, op: &'static str, i: &Ins) {
        if i.sig_values().iter().any(|(n, _, _)| n.starts_with('v')) {
            let v = self.reg_of(i);
            let _ = self.v(v);
        }
        let e = self.take_acc();
        self.set_acc(Expr::Un(op, Box::new(e)));
    }

    fn update(&mut self, op: &'static str, i: &Ins, post: bool) {
        let v = self.reg_of(i);
        let e = self.v(v);
        self.set_acc(Expr::Update(op, Box::new(e), post));
    }

    fn v_target(&self, n: u64) -> Expr {
        if (n as u32) >= self.num_vregs && (n as u32) < self.num_vregs + self.num_args {
            Expr::Arg(n as u32 - self.num_vregs)
        } else {
            Expr::Reg(n as u32)
        }
    }
}

/// 表达式是否可能调用用户代码或抛错（丢弃值仍须保留求值及异常范围）。
pub fn has_effect(e: &Expr) -> bool {
    match e {
        // 调用本身就是副作用
        Expr::Call { .. } | Expr::Update(_, _, _) | Expr::Assign(_, _) | Expr::Await(_) => true,
        // 数值/关系/宽松相等运算会 ToPrimitive，可能执行 valueOf/toString。
        // in/instanceof 也可能抛错或调用 @@hasInstance，不能只检查子表达式。
        Expr::Bin(op, a, b) => {
            !matches!(*op, "===" | "!==" | "&&" | "||" | "??") || has_effect(a) || has_effect(b)
        }
        // ToBoolean 和 typeof 不转换对象；数值转换与其余一元运算保守保留。
        Expr::Un(op, a) => !matches!(*op, "!" | "typeof" | "Boolean" | "void") || has_effect(a),
        // 属性读取可能触发 getter / Proxy，不能当作纯读取丢弃。
        Expr::Member(_, _) | Expr::Index(_, _) => true,
        Expr::Cond(c, a, b) => has_effect(c) || has_effect(a) || has_effect(b),
        // 数组展开会执行 iterator 协议，即使源表达式只是标识符也可能有副作用。
        Expr::Spread(_) => true,
        Expr::Object(fields) => fields.iter().any(|(_, value)| has_effect(value)),
        Expr::Array(elements) => elements.iter().any(has_effect),
        _ => false,
    }
}

/// 用计算属性语法执行 ToPropertyKey，保留 Symbol 与 string 提示的 ToPrimitive。
fn to_property_key(key: Expr) -> Expr {
    call_expr(
        Expr::TemplateLit("((key) => globalThis.Reflect.ownKeys({ [key]: 0 })[0])".into()),
        vec![key],
    )
}

/// CreateDataPropertyOrThrow 的标准属性标志。
fn define_data_property(receiver: Expr, key: Expr, value: Expr) -> Expr {
    call_expr(
        Expr::Special("globalThis.Object.defineProperty"),
        vec![
            receiver,
            key,
            Expr::Object(vec![
                ("value".into(), value),
                ("writable".into(), Expr::Bool(true)),
                ("enumerable".into(), Expr::Bool(true)),
                ("configurable".into(), Expr::Bool(true)),
            ]),
        ],
    )
}

fn call_expr(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        new: false,
        this_arg: None,
    }
}

/// 取 imm 的有符号值
fn signed_imm_of(i: &Ins) -> i64 {
    let raw = i.field("imm").unwrap_or(0);
    let bits = i
        .meta
        .fields
        .iter()
        .find(|f| f.name == "imm")
        .map(|f| f.bits)
        .unwrap_or(32);
    match bits {
        8 => raw as u8 as i8 as i64,
        16 => raw as u16 as i16 as i64,
        _ => raw as u32 as i32 as i64,
    }
}

/// 取前两个 imm（如 ldlexvar depth, slot）
fn two_imms(i: &Ins) -> (u64, u64) {
    let mut vals = Vec::new();
    for (n, _, v) in i.sig_values() {
        if n.starts_with("imm") {
            vals.push(v);
        }
    }
    (
        vals.first().copied().unwrap_or(0),
        vals.get(1).copied().unwrap_or(0),
    )
}

/// 条件取反（jeqz 语义）
pub(crate) fn invert_cond(e: Expr) -> Expr {
    match e {
        Expr::Un("!", inner) => *inner,
        Expr::Bool(value) => Expr::Bool(!value),
        Expr::Bin(op, a, b) if matches!(op, "===" | "!==" | "==" | "!=") => {
            let opposite = match op {
                "===" => "!==",
                "!==" => "===",
                "==" => "!=",
                _ => "==",
            };
            Expr::Bin(opposite, a, b)
        }
        other => Expr::Un("!", Box::new(other)),
    }
}

#[cfg(test)]
mod async_tests {
    use super::*;
    use crate::test_support::read_fixture;

    #[test]
    fn module_namespace_uses_request_index_not_namespace_array_position() {
        let module = ModuleRecord {
            namespace_imports: vec![crate::file::NamespaceImport {
                local_name: "SheetApi".into(),
                module_idx: 7,
            }],
            ..ModuleRecord::default()
        };
        assert_eq!(
            namespace_name_for_request(Some(&module), 7),
            Some("SheetApi")
        );
        assert_eq!(namespace_name_for_request(Some(&module), 0), None);
    }

    #[test]
    fn external_module_variable_uses_the_import_binding_name() {
        let module = ModuleRecord {
            regular_imports: vec![crate::file::RegularImport {
                local_name: "PafSheetConfig".into(),
                import_name: "PafSheetConfig".into(),
                module_idx: 2,
            }],
            ..ModuleRecord::default()
        };
        assert_eq!(
            external_import_name(Some(&module), 0),
            Some("PafSheetConfig")
        );
        assert_eq!(external_import_name(Some(&module), 1), None);
    }

    #[test]
    fn compiler_async_protocols_include_exception_and_loop_entries() {
        for path in [
            "advanced_recovery/debug.abc",
            "advanced_recovery/release.abc",
        ] {
            let bytes = read_fixture(path);
            let file = AbcFile::parse(bytes).unwrap();
            for name in [
                "awaitSuccess",
                "awaitFailure",
                "catchFinally",
                "finallyOverridesReturn",
                "nestedFinally",
                "catchThrows",
                "awaitLoop",
                "awaitInFinally",
            ] {
                let method = file
                    .classes
                    .iter()
                    .flat_map(|c| &c.methods)
                    .find(|m| m.name == name)
                    .unwrap();
                let code = method.code.as_ref().unwrap();
                let ins = crate::disasm::decode(code.instructions(&file.data)).unwrap();
                let cfg =
                    crate::decompile::cfg::build(&ins, code.code_size as usize, &code.try_blocks);
                assert!(
                    normalize_async(&ins, &code.try_blocks, &cfg).is_some(),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn altered_resume_mode_uses_complete_raw_fallback() {
        let bytes = read_fixture("advanced_recovery/debug.abc");
        let mut file = AbcFile::parse(bytes).unwrap();
        let class = file
            .classes
            .iter()
            .find(|c| c.methods.iter().any(|m| m.name == "awaitSuccess"))
            .unwrap()
            .clone();
        let method = class
            .methods
            .iter()
            .find(|m| m.name == "awaitSuccess")
            .unwrap();
        let code = method.code.as_ref().unwrap();
        let ins = crate::disasm::decode(code.instructions(&file.data)).unwrap();
        let equal = ins.iter().find(|i| i.meta.mnemonic == "eq").unwrap();
        let different = crate::isa_gen::INSTRUCTIONS
            .iter()
            .find(|i| i.mnemonic == "noteq" && i.size == equal.size)
            .unwrap();
        file.data[code.ins_off as usize + equal.pc] = different.opcode;
        let ins = crate::disasm::decode(code.instructions(&file.data)).unwrap();
        let cfg = crate::decompile::cfg::build(&ins, code.code_size as usize, &code.try_blocks);
        let body = lift(&file, &class, method, code, &cfg, None);
        assert!(!body.is_async);
        let [Stmt::Comment(text)] = body.stmts.as_slice() else {
            panic!("expected complete fallback")
        };
        assert!(
            text.contains("Conservative fallback")
                && text.contains(".catchall")
                && text.contains("asyncfunctionreject")
                && text.contains("getresumemode")
        );
        assert!(!text.contains("*/"));
    }

    #[test]
    fn finally_recognition_requires_the_internal_hole_sentinel() {
        let bytes = read_fixture("advanced_recovery/debug.abc");
        let file = AbcFile::parse(bytes).unwrap();
        let method = file
            .classes
            .iter()
            .flat_map(|c| &c.methods)
            .find(|m| m.name == "catchFinally")
            .unwrap();
        let code = method.code.as_ref().unwrap();
        let mut ins = crate::disasm::decode(code.instructions(&file.data)).unwrap();
        let handler = code
            .try_blocks
            .iter()
            .flat_map(|t| &t.catch_blocks)
            .map(|c| c.handler_pc as usize)
            .find(|pc| finally_cleanup(&ins, *pc).is_some())
            .unwrap();
        let undefined = crate::isa_gen::INSTRUCTIONS
            .iter()
            .find(|i| i.mnemonic == "ldundefined")
            .unwrap();
        for instruction in &mut ins {
            if instruction.meta.mnemonic == "ldhole" {
                instruction.meta = undefined;
            }
        }
        assert!(finally_cleanup(&ins, handler).is_none());
    }
}

#[cfg(test)]
#[path = "async_download_recovery_tests.rs"]
mod async_download_recovery_tests;
