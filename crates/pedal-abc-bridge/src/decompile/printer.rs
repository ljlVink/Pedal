//! 伪 ArkTS 打印：表达式/语句 → 文本；模块 import/export 渲染
//!
//! @author ljlVink

use crate::decompile::lift::{Expr, Stmt};
use crate::file::{AbcFile, ClassInfo, CodeInfo, LiteralValue, MethodInfo};
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// 渲染上下文：持有文件以便展开 literal。
/// literal 缓存兼作循环守卫：literal 数组可自引用/共享，
/// 不加记忆化会在 DAG 展开时指数爆炸
/// 嵌套函数（闭包/类成员）展开缓存，单次方法渲染内共享
pub struct NestedCache {
    pub(crate) recovery: Option<std::rc::Rc<super::class_recovery::RecordRecovery>>,
    pub(crate) map: HashMap<u64, String>,
    /// 闭包展开剩余额度 / 已产字节
    pub(crate) closure_budget: usize,
    pub(crate) closure_bytes: usize,
    /// 类成员展开剩余额度 / 已产字节
    pub(crate) member_budget: usize,
    pub(crate) member_bytes: usize,
    /// 实际嵌入正文的闭包总字节数；缓存命中也计入，避免乘性膨胀。
    pub(crate) closure_emitted_bytes: usize,
}

impl NestedCache {
    /// Promise、then 与集合回调常形成多层嵌套，仍以深度及总额度共同限制展开。
    pub const MAX_DEPTH: u8 = 8;
    /// 单 record 闭包/类成员展开文本上限
    pub const MAX_CLOSURE_BYTES: usize = 1024 * 1024;
    pub const MAX_MEMBER_BYTES: usize = 64 * 1024;

    /// 为本次闭包表达式保留输出额度；相同方法可以在多个表达式中展开。
    pub(crate) fn embed_closure(&mut self, len: usize) -> bool {
        if self.closure_emitted_bytes.saturating_add(len) > Self::MAX_CLOSURE_BYTES {
            return false;
        }
        self.closure_emitted_bytes += len;
        true
    }

    pub(crate) fn exhausted(&self, arrow: bool) -> bool {
        if arrow {
            self.closure_budget == 0 || self.closure_bytes >= Self::MAX_CLOSURE_BYTES
        } else {
            self.member_budget == 0 || self.member_bytes >= Self::MAX_MEMBER_BYTES
        }
    }

    pub(crate) fn reserve(&mut self, arrow: bool) {
        if arrow {
            self.closure_budget = self.closure_budget.saturating_sub(1);
        } else {
            self.member_budget = self.member_budget.saturating_sub(1);
        }
    }

    pub(crate) fn record_bytes(&mut self, arrow: bool, len: usize) {
        if arrow {
            self.closure_bytes = self.closure_bytes.saturating_add(len);
        } else {
            self.member_bytes = self.member_bytes.saturating_add(len);
        }
    }
}

pub struct Renderer<'a> {
    /// 宿主 abc 文件
    pub file: &'a AbcFile,
    /// literal 数组渲染缓存：key = off*2 + is_class；None = 渲染中（环）
    lit_cache: RefCell<HashMap<u32, Option<String>>>,
    /// 嵌套函数展开共享缓存
    nested: std::rc::Rc<RefCell<NestedCache>>,
    /// 闭包/类成员递归反编译深度
    depth: u8,
    /// 实例方法：a0 渲染为 this 且不出现在参数列表
    this_arg0: bool,
    /// 现代 Ark ABI 的前三个参数是 function / new.target / this
    modern_abi: bool,
    /// 当前语句缩进，供多行闭包表达式调整续行
    indent_depth: Cell<usize>,
    /// 只对已确认的调试绑定命名，不替换字面量或任意文本。
    names: NameContext,
}

/// 方法内名字表；无法证明整个寄存器属于同一个局部时保留寄存器名。
#[derive(Default)]
struct NameContext {
    args: HashMap<u32, String>,
    arg_types: HashMap<u32, String>,
    return_type: Option<String>,
    arkui_inferred: bool,
    regs: HashMap<u32, String>,
    /// 入口参数搬运到局部槽，且原参数槽在之后不再使用。
    parameter_copies: HashMap<u32, u32>,
    /// 可用于签名角色识别的入口副本；不满足全方法别名条件时不改正文绑定。
    parameter_roles: HashMap<u32, u32>,
}

impl NameContext {
    fn for_method(
        file: &AbcFile,
        method: &MethodInfo,
        code: &CodeInfo,
        body: &crate::decompile::lift::LiftedBody,
        first_param: usize,
    ) -> Self {
        let mut names = Self::default();
        if let Some((params, ret)) = super::signatures::metadata(file, method) {
            names.return_type = ret;
            if params.len() == code.num_args as usize - first_param {
                for (index, ty) in params.into_iter().enumerate() {
                    if let Some(ty) = ty {
                        names.arg_types.insert((index + first_param) as u32, ty);
                    }
                }
            }
        }
        let Ok(Some(debug)) = file.read_debug_info(method) else {
            return names;
        };
        let modern = file.header.kind == crate::file::HeaderKind::Modern;
        let mut local_counts = HashMap::<i32, usize>::new();
        for local in &debug.locals {
            *local_counts.entry(local.register).or_default() += 1;
        }
        let mut reserved = HashSet::from([
            "currentFunction".to_string(),
            "undefined".to_string(),
            "NaN".to_string(),
            "Infinity".to_string(),
            "arguments".to_string(),
            "globalThis".to_string(),
        ]);
        let mut used_regs = Vec::new();
        for stmt in &body.stmts {
            collect_stmt_regs(stmt, &mut used_regs);
            collect_stmt_names(stmt, &mut reserved);
        }
        let fixed_names = reserved.clone();
        for reg in used_regs {
            reserved.insert(format!("v{reg}"));
        }
        for arg in first_param..code.num_args as usize {
            reserved.insert(default_arg(arg as u32, modern));
        }
        let parameter_start = if debug.parameter_names.len() == code.num_args as usize {
            Some(0)
        } else if debug.parameter_names.len() == code.num_args as usize - first_param {
            Some(first_param)
        } else {
            None
        };
        if let Some(parameter_start) = parameter_start {
            for (index, candidate) in debug.parameter_names.iter().enumerate() {
                let arg = index + parameter_start;
                if arg < first_param {
                    continue;
                }
                if let Some(candidate) = candidate
                    && valid_binding(candidate)
                {
                    let fallback = default_arg(arg as u32, modern);
                    let name = unique_binding(candidate, &fallback, &mut reserved, &fixed_names);
                    names.args.insert(arg as u32, name);
                }
            }
        }
        // 有些版本把参数名称也放进 START_LOCAL；只有覆盖整个方法的参数槽才采纳。
        for local in &debug.locals {
            if local.register >= code.num_vregs as i32
                && (local.register as u32) < code.num_vregs + code.num_args
                && local.start_pc == 0
                && local.end_pc == code.code_size
            {
                let arg = local.register as u32 - code.num_vregs;
                if arg as usize >= first_param
                    && !names.args.contains_key(&arg)
                    && valid_binding(&local.name)
                    && local_counts.get(&local.register) == Some(&1)
                {
                    let name = unique_binding(
                        &local.name,
                        &default_arg(arg, modern),
                        &mut reserved,
                        &fixed_names,
                    );
                    names.args.insert(arg, name);
                }
            }
        }
        let Ok(instructions) = crate::disasm::decode_with(code.instructions(file.bytes()), !modern)
        else {
            return names;
        };
        for local in &debug.locals {
            if local_counts.get(&local.register) != Some(&1) {
                continue;
            }
            let arg = if local.register >= code.num_vregs as i32
                && (local.register as u32) < code.num_vregs + code.num_args
                && local.start_pc == 0
                && local.end_pc == code.code_size
            {
                Some(local.register as u32 - code.num_vregs)
            } else {
                parameter_copy(local, code, &instructions).map(|(arg, _, _)| arg)
            };
            if let Some(arg) = arg.filter(|arg| *arg as usize >= first_param)
                && let Some(ty) = local
                    .signature
                    .as_deref()
                    .and_then(super::signatures::debug_type)
                    .or_else(|| {
                        local
                            .type_name
                            .as_deref()
                            .and_then(super::signatures::debug_type)
                    })
            {
                names.arg_types.entry(arg).or_insert(ty);
            }
        }
        // es2abc 的调试构建通常省略参数表，改为入口 mov 后对局部槽发 START_LOCAL。
        // 通过真实数据流建立参数对应关系，不能按 v3 等寄存器号直接推断参数位置。
        for local in &debug.locals {
            if !valid_binding(&local.name) || local_counts.get(&local.register) != Some(&1) {
                continue;
            }
            let Some((arg, copy_pc, can_alias)) = parameter_copy(local, code, &instructions) else {
                continue;
            };
            if (arg as usize) < first_param {
                continue;
            }
            names.parameter_roles.insert(local.register as u32, arg);
            let name = names
                .args
                .entry(arg)
                .or_insert_with(|| {
                    unique_binding(
                        &local.name,
                        &default_arg(arg, modern),
                        &mut reserved,
                        &fixed_names,
                    )
                })
                .clone();
            if can_alias && local_covers_register_from(local, &instructions, Some(copy_pc)) {
                names.parameter_copies.insert(local.register as u32, arg);
                names.regs.insert(local.register as u32, name);
            }
        }
        for local in &debug.locals {
            if local.register < 0
                || local.register as u32 >= code.num_vregs
                || names
                    .parameter_copies
                    .contains_key(&(local.register as u32))
                || !valid_binding(&local.name)
                || local_counts.get(&local.register) != Some(&1)
                || !local_covers_register(local, &instructions)
            {
                continue;
            }
            let reg = local.register as u32;
            let name = unique_binding(&local.name, &format!("v{reg}"), &mut reserved, &fixed_names);
            names.regs.insert(reg, name);
        }
        names
    }

    fn reg(&self, reg: u32) -> String {
        self.regs
            .get(&reg)
            .cloned()
            .unwrap_or_else(|| format!("v{reg}"))
    }

    fn arg(&self, arg: u32, modern: bool) -> String {
        self.args
            .get(&arg)
            .cloned()
            .unwrap_or_else(|| default_arg(arg, modern))
    }

    fn parameter(&self, arg: u32, modern: bool) -> String {
        let name = self.arg(arg, modern);
        self.arg_types
            .get(&arg)
            .map_or_else(|| name.clone(), |ty| format!("{name}: {ty}"))
    }

    fn return_annotation(&self, constructor: bool) -> String {
        if constructor {
            return String::new();
        }
        self.return_type
            .as_ref()
            .map_or_else(String::new, |ty| format!(": {ty}"))
    }

    fn apply_arkui_constructor(&mut self, body: &crate::decompile::lift::LiftedBody) {
        let argument = |expr: &Expr| match expr {
            Expr::Arg(arg) => Some(*arg),
            Expr::Reg(reg) => self.parameter_roles.get(reg).copied(),
            _ => None,
        };
        let mut super_call = false;
        let mut initial_params = false;
        let mut params_generator = false;
        super::class_recovery::visit_stmts(&mut body.stmts.clone(), &mut |expr| match expr {
            Expr::Call { callee, args, .. } => {
                if matches!(callee.as_ref(), Expr::Special("super"))
                    && args.len() == 4
                    && args
                        .iter()
                        .map(&argument)
                        .eq([Some(3), Some(5), Some(6), Some(8)])
                {
                    super_call = true;
                }
                if matches!(callee.as_ref(), Expr::Member(_, prop) if prop == "setInitiallyProvidedValue")
                    && args.len() == 1
                    && argument(&args[0]) == Some(4)
                {
                    initial_params = true;
                }
            }
            Expr::Assign(target, value)
                if matches!(target.as_ref(), Expr::Member(_, prop) if prop == "paramsGenerator_")
                    && argument(value) == Some(7) =>
            {
                params_generator = true;
            }
            _ => {}
        });
        if !(super_call && initial_params && params_generator) {
            return;
        }

        let mut reserved: HashSet<String> = self
            .args
            .values()
            .chain(self.regs.values())
            .cloned()
            .collect();
        for stmt in &body.stmts {
            collect_stmt_names(stmt, &mut reserved);
        }
        let fixed = reserved.clone();
        for (index, (candidate, ty)) in super::signatures::ARKUI_PARAMETERS.iter().enumerate() {
            let arg = index as u32 + 3;
            if let std::collections::hash_map::Entry::Vacant(entry) = self.args.entry(arg) {
                let name =
                    unique_binding(candidate, &default_arg(arg, true), &mut reserved, &fixed);
                entry.insert(name);
            }
            // 原有元数据优先；框架只能提供角色，params 的具体接口已被擦除。
            if let std::collections::hash_map::Entry::Vacant(entry) = self.arg_types.entry(arg) {
                entry.insert((*ty).into());
                self.arkui_inferred = true;
            }
        }
    }
}

fn first_parameter(file: &AbcFile, method: &MethodInfo, code: &CodeInfo, fallback: usize) -> usize {
    if file.header.kind == crate::file::HeaderKind::Modern {
        return 3.min(code.num_args as usize);
    }
    if let Some((params, _)) = super::signatures::metadata(file, method) {
        if params.len() == code.num_args as usize {
            return 0;
        }
        if params.len() + 1 == code.num_args as usize {
            return 1;
        }
    }
    fallback.min(code.num_args as usize)
}

fn parameter_list(params: &[String]) -> String {
    let inline = params.join(", ");
    if inline.len() <= 100 {
        inline
    } else {
        format!("\n    {},\n", params.join(",\n    "))
    }
}

fn default_arg(arg: u32, modern: bool) -> String {
    format!("a{}", if modern { arg.saturating_sub(3) } else { arg })
}

fn valid_binding(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || matches!(c, '_' | '$'))
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'))
        && !matches!(
            name,
            "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "debugger"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "enum"
                | "export"
                | "extends"
                | "false"
                | "finally"
                | "for"
                | "function"
                | "if"
                | "import"
                | "in"
                | "instanceof"
                | "let"
                | "new"
                | "null"
                | "return"
                | "super"
                | "switch"
                | "this"
                | "throw"
                | "true"
                | "try"
                | "typeof"
                | "var"
                | "void"
                | "while"
                | "with"
                | "yield"
                | "implements"
                | "interface"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "static"
        )
}

/// 对象字面量的属性名保留其数据属性语义，并引用非标识符键。
fn object_property_key(name: &str) -> String {
    if name == "__proto__" {
        "[\"__proto__\"]".into()
    } else if valid_binding(name) {
        name.into()
    } else {
        format!("{name:?}")
    }
}

fn unique_binding(
    candidate: &str,
    fallback: &str,
    used: &mut HashSet<String>,
    fixed: &HashSet<String>,
) -> String {
    if (candidate == fallback && !fixed.contains(candidate)) || used.insert(candidate.to_owned()) {
        return candidate.to_owned();
    }
    let base = format!("{candidate}_{fallback}");
    let mut name = base.clone();
    let mut suffix = 2;
    while !used.insert(name.clone()) {
        name = format!("{base}_{suffix}");
        suffix += 1;
    }
    name
}

fn local_covers_register(
    local: &crate::file::debug::LocalVariable,
    ins: &[crate::disasm::Ins],
) -> bool {
    local_covers_register_from(local, ins, None)
}

fn local_covers_register_from(
    local: &crate::file::debug::LocalVariable,
    ins: &[crate::disasm::Ins],
    parameter_copy_pc: Option<usize>,
) -> bool {
    let mut used = false;
    for i in ins {
        let pc = i.pc as u32;
        let in_scope = local.start_pc <= pc && pc < local.end_pc;
        // range 指令可隐式读取多个寄存器，范围外出现时保守放弃该名字。
        if i.meta.mnemonic.contains("range") && !in_scope {
            return false;
        }
        for (operand, _, value) in i.sig_values() {
            if operand.starts_with('v') && value == local.register as u64 {
                used = true;
                let initialization = pc + u32::from(i.size) == local.start_pc
                    && matches!(i.meta.mnemonic, "sta" | "sta.16")
                    || parameter_copy_pc == Some(i.pc);
                if !in_scope && !initialization {
                    return false;
                }
            }
        }
    }
    used
}

fn parameter_copy(
    local: &crate::file::debug::LocalVariable,
    code: &CodeInfo,
    instructions: &[crate::disasm::Ins],
) -> Option<(u32, usize, bool)> {
    if local.register < 0 || local.end_pc != code.code_size {
        return None;
    }
    let reg = local.register as u64;
    let mut copy = None;
    for ins in instructions
        .iter()
        .take_while(|i| (i.pc as u32) < local.start_pc)
    {
        let operands = ins.sig_values();
        if operands.iter().any(|(name, decl, value)| {
            name.starts_with('v') && *value == reg && decl.starts_with("out")
        }) {
            if copy.is_some() || !matches!(ins.meta.mnemonic, "mov" | "mov.16") {
                return None;
            }
            let source = operands.get(1)?.2;
            if source < code.num_vregs as u64 || source >= (code.num_vregs + code.num_args) as u64 {
                return None;
            }
            copy = Some((source as u32 - code.num_vregs, ins.pc));
        }
    }
    let (arg, copy_pc) = copy?;
    let source = u64::from(code.num_vregs + arg);
    let can_alias = !instructions.iter().any(|ins| {
        if ins.pc == copy_pc {
            return false;
        }
        let operands = ins.sig_values();
        operands
            .iter()
            .any(|(name, _, value)| name.starts_with('v') && *value == source)
            || (ins.meta.mnemonic.contains("range")
                && operands.iter().any(|(name, _, start)| {
                    if !name.starts_with('v') || *start > source {
                        return false;
                    }
                    let count = operands
                        .iter()
                        .filter(|(name, _, _)| name.starts_with("imm"))
                        .map(|(_, _, value)| *value)
                        .max()
                        .unwrap_or(0);
                    source < start.saturating_add(count).saturating_add(2)
                }))
    });
    Some((arg, copy_pc, can_alias))
}

fn collect_stmt_names(stmt: &Stmt, names: &mut HashSet<String>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => collect_expr_names(e, names),
        Stmt::Declare(name, init) => {
            names.insert(name.clone());
            if let Some(e) = init {
                collect_expr_names(e, names);
            }
        }
        Stmt::If(cond, body, other) => {
            collect_expr_names(cond, names);
            for stmt in body.iter().chain(other) {
                collect_stmt_names(stmt, names);
            }
        }
        Stmt::While(cond, body) | Stmt::DoWhile(body, cond) => {
            collect_expr_names(cond, names);
            for stmt in body {
                collect_stmt_names(stmt, names);
            }
        }
        Stmt::ForOf(name, iter, body) => {
            names.insert(name.clone());
            collect_expr_names(iter, names);
            for stmt in body {
                collect_stmt_names(stmt, names);
            }
        }
        Stmt::Try(body, catches) => {
            for stmt in body {
                collect_stmt_names(stmt, names);
            }
            for (name, body) in catches {
                if let Some(name) = name {
                    names.insert(name.clone());
                }
                for stmt in body {
                    collect_stmt_names(stmt, names);
                }
            }
        }
        Stmt::TryFinally(body, finalizer) => {
            for stmt in body.iter().chain(finalizer) {
                collect_stmt_names(stmt, names);
            }
        }
        _ => {}
    }
}

fn collect_expr_names(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::Object(fields) => {
            for (_, value) in fields {
                collect_expr_names(value, names);
            }
        }
        Expr::Array(elements) => {
            for value in elements {
                collect_expr_names(value, names);
            }
        }
        Expr::Name(name) => {
            names.insert(name.clone());
        }
        Expr::Bin(_, a, b) | Expr::Assign(a, b) | Expr::Index(a, b) => {
            collect_expr_names(a, names);
            collect_expr_names(b, names);
        }
        Expr::Un(_, a)
        | Expr::Update(_, a, _)
        | Expr::Member(a, _)
        | Expr::Spread(a)
        | Expr::Await(a) => collect_expr_names(a, names),
        Expr::Cond(c, a, b) => {
            collect_expr_names(c, names);
            collect_expr_names(a, names);
            collect_expr_names(b, names);
        }
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            collect_expr_names(callee, names);
            for arg in args {
                collect_expr_names(arg, names);
            }
            if let Some(this_arg) = this_arg {
                collect_expr_names(this_arg, names);
            }
        }
        _ => {}
    }
}

/// 无方法体时仍保留 ABC 提供的参数和返回值声明。
pub(super) fn render_declaration(file: &AbcFile, method: &MethodInfo) -> Option<String> {
    let (params, ret) = super::signatures::metadata(file, method)?;
    let name = demangle_or_offset(&method.name, method.offset);
    let params = params
        .iter()
        .enumerate()
        .map(|(index, ty)| format!("a{index}: {}", ty.as_deref().unwrap_or("unknown")))
        .collect::<Vec<_>>()
        .join(", ");
    let ret = ret.map_or_else(String::new, |ty| format!(": {ty}"));
    Some(format!(
        "// {} @ {:#x}: declaration only; no bytecode body\ndeclare function {name}({params}){ret};\n",
        method.name, method.offset
    ))
}

/// 渲染整个方法（含函数头）
pub fn render_method(
    file: &AbcFile,
    _class: &ClassInfo,
    m: &MethodInfo,
    code: &CodeInfo,
    body: &crate::decompile::lift::LiftedBody,
    nested: &std::rc::Rc<RefCell<NestedCache>>,
) -> String {
    let mut out = String::with_capacity(256);
    let name = if m.name == "<ctor>" {
        "constructor".to_string()
    } else {
        demangle_or_offset(&m.name, m.offset)
    };
    let modifier = match m.kind {
        crate::file::FunctionKind::GeneratorFunction => "*",
        crate::file::FunctionKind::AsyncFunction | crate::file::FunctionKind::AsyncNcFunction => {
            "async "
        }
        crate::file::FunctionKind::AsyncGeneratorFunction => "async *",
        crate::file::FunctionKind::ConcurrentFunction if body.is_async => "async ",
        crate::file::FunctionKind::ConcurrentFunction if body.is_concurrent => "",
        crate::file::FunctionKind::ConcurrentFunction => "/* @Concurrent */ ",
        _ if body.is_async => "async ",
        _ => "",
    };
    // 实例方法 a0 是 this（参数从 a1 起）；类静态方法 a0..a2 是
    // (env, 类引用, ...) 机器参数（参数从 a3 起）
    let class_static = m.name.starts_with("#&@") && m.name.contains('*');
    let modern_abi = file.header.kind == crate::file::HeaderKind::Modern;
    let legacy_this = !class_static && m.name.contains('<');
    let fallback = if class_static {
        3.min(code.num_args as usize)
    } else {
        usize::from(legacy_this)
    };
    let first_param = first_parameter(file, m, code, fallback);
    let this_arg0 = !modern_abi && first_param == 1;
    let names = NameContext::for_method(file, m, code, body, first_param);
    let params: Vec<String> = (first_param..code.num_args as usize)
        .map(|i| names.parameter(i as u32, modern_abi))
        .collect();
    out.push_str(&format!("// {} @ {:#x}\n", m.name, m.offset));
    out.push_str(&format!(
        "{modifier}function {name}({}){} {{\n",
        parameter_list(&params),
        names.return_annotation(name == "constructor")
    ));
    let renderer = Renderer {
        file,
        lit_cache: RefCell::new(HashMap::new()),
        nested: nested.clone(),
        depth: 0,
        this_arg0,
        modern_abi,
        indent_depth: Cell::new(0),
        names,
    };
    renderer.render_body(&body.stmts, body.is_concurrent, 1, &mut out);
    out.push_str("}\n");
    let _ = file;
    out
}

/// 嵌套函数渲染（闭包/类成员）。depth 传递给内部表达式以限递归。
pub fn render_nested(
    file: &AbcFile,
    m: &MethodInfo,
    code: &CodeInfo,
    body: &crate::decompile::lift::LiftedBody,
    depth: u8,
    style: crate::decompile::NestedStyle,
    nested: &std::rc::Rc<RefCell<NestedCache>>,
) -> String {
    let modern_abi = file.header.kind == crate::file::HeaderKind::Modern;
    let fallback = if matches!(style, crate::decompile::NestedStyle::Method) {
        usize::from(m.name.contains('<'))
    } else {
        0
    };
    let first_param = first_parameter(file, m, code, fallback);
    let this_arg0 = !modern_abi && first_param == 1;
    let mut names = NameContext::for_method(file, m, code, body, first_param);
    if modern_abi
        && code.num_args == 9
        && nested
            .borrow()
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.is_arkui_constructor(m.offset))
    {
        names.apply_arkui_constructor(body);
    }
    let renderer = Renderer {
        file,
        lit_cache: RefCell::new(HashMap::new()),
        nested: nested.clone(),
        depth,
        this_arg0,
        modern_abi,
        indent_depth: Cell::new(0),
        names,
    };
    let async_prefix = if body.is_async
        || matches!(
            m.kind,
            crate::file::FunctionKind::AsyncFunction
                | crate::file::FunctionKind::AsyncNcFunction
                | crate::file::FunctionKind::AsyncGeneratorFunction
        ) {
        "async "
    } else {
        ""
    };
    match style {
        crate::decompile::NestedStyle::Arrow => {
            let params: Vec<String> = (first_param..code.num_args as usize)
                .map(|i| renderer.names.parameter(i as u32, renderer.modern_abi))
                .collect();
            // 实例初始化器经 callinit 以显式接收者调用，必须保留动态 this。
            let head = if m.name.contains("instance_initializer") {
                format!(
                    "{async_prefix}function({}){} {{\n",
                    parameter_list(&params),
                    renderer.names.return_annotation(false)
                )
            } else {
                format!(
                    "{async_prefix}({}){} => {{\n",
                    parameter_list(&params),
                    renderer.names.return_annotation(false)
                )
            };
            let mut out = head;
            renderer.render_body(&body.stmts, body.is_concurrent, 1, &mut out);
            out.push('}');
            out
        }
        crate::decompile::NestedStyle::Method => {
            let name = if m.name == "<ctor>" {
                "constructor".to_string()
            } else {
                demangle_or_offset(&m.name, m.offset)
            };
            let params: Vec<String> = (first_param..code.num_args as usize)
                .map(|i| renderer.names.parameter(i as u32, renderer.modern_abi))
                .collect();
            let generator = if matches!(
                m.kind,
                crate::file::FunctionKind::GeneratorFunction
                    | crate::file::FunctionKind::AsyncGeneratorFunction
            ) {
                "*"
            } else {
                ""
            };
            let mut out = format!(
                "{async_prefix}{generator}{name}({}){} {{\n",
                parameter_list(&params),
                renderer.names.return_annotation(name == "constructor")
            );
            if renderer.names.arkui_inferred {
                out.insert_str(0, "// Parameter roles/types inferred from ArkUI ViewPU; unknown = erased component params type.\n");
            }
            renderer.render_body(&body.stmts, body.is_concurrent, 1, &mut out);
            out.push('}');
            out
        }
    }
}

/// 模块顶层语句渲染（func_main_0 展开）
pub fn render_top_level(
    file: &AbcFile,
    body: &crate::decompile::lift::LiftedBody,
    nested: &std::rc::Rc<RefCell<NestedCache>>,
) -> String {
    let renderer = Renderer {
        file,
        lit_cache: RefCell::new(HashMap::new()),
        nested: nested.clone(),
        depth: 0,
        this_arg0: false,
        modern_abi: file.header.kind == crate::file::HeaderKind::Modern,
        indent_depth: Cell::new(0),
        names: NameContext::default(),
    };
    let mut out = String::new();
    renderer.render_body(&body.stmts, body.is_concurrent, 0, &mut out);
    out
}

/// 清理方法名显示：`foo#bar.ets#3` → `foo`；
/// 编译器合成名（无合法标识符）降级为 `syn_&lt;offset&gt;`
pub fn clean_method_name(name: &str) -> String {
    // es2abc 混淆名：`#~@12<#member` / `#~@12=#Ctor` / `foo#file.ets#3`
    if name.starts_with('#') || name.contains('#') {
        if let Some(last) = name.rsplit('#').next()
            && !last.is_empty()
            && !last.contains('.')
        {
            return sanitize_ident(last);
        }
        if let Some(first) = name.split('#').next()
            && !first.is_empty()
        {
            return sanitize_ident(first);
        }
    }
    sanitize_ident(name)
}

fn sanitize_ident(name: &str) -> String {
    if name.is_empty() {
        return "<anonymous>".into();
    }
    let ok = name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '$');
    if ok {
        name.to_string()
    } else {
        format!("{name:?}")
    }
}

/// 合成名降级：`#~@0>@4*#` 这类无法还原的标识符改用偏移命名
pub fn demangle_or_offset(name: &str, offset: u32) -> String {
    let cleaned = clean_method_name(name);
    if cleaned.starts_with('"') || cleaned == "<anonymous>" {
        format!("syn_{offset:#x}")
    } else {
        cleaned
    }
}

fn indent(n: usize) -> String {
    "    ".repeat(n)
}

/// 寄存器按整个方法收集；仅在第一次出现就是同层独立赋值时合并声明。
/// 分支/循环/异常处理中的读写同样计入，右侧自引用和参数副本保持原写法。
fn local_declarations(stmts: &[Stmt], names: &NameContext) -> (Vec<u32>, HashSet<usize>) {
    let mut seen = HashSet::new();
    let mut initialized = HashSet::new();
    let mut initializers = HashSet::new();
    let mut refs = Vec::new();
    for (index, stmt) in stmts.iter().enumerate() {
        refs.clear();
        collect_stmt_regs(stmt, &mut refs);
        if let Stmt::Expr(Expr::Assign(target, _)) = stmt
            && let Expr::Reg(reg) = target.as_ref()
            && !names.parameter_copies.contains_key(reg)
            && !seen.contains(reg)
            && refs.iter().filter(|r| *r == reg).count() == 1
        {
            initialized.insert(*reg);
            initializers.insert(index);
        }
        seen.extend(refs.iter().copied());
    }
    let mut regs: Vec<_> = seen
        .into_iter()
        .filter(|reg| !initialized.contains(reg) && !names.parameter_copies.contains_key(reg))
        .collect();
    regs.sort_unstable();
    (regs, initializers)
}

fn write_local_declaration(regs: &[u32], names: &NameContext, depth: usize, out: &mut String) {
    if !regs.is_empty() {
        out.push_str(&indent(depth));
        out.push_str("let ");
        for (i, reg) in regs.iter().enumerate() {
            if i != 0 {
                out.push_str(", ");
            }
            out.push_str(&names.reg(*reg));
        }
        out.push_str(";\n");
    }
}

fn collect_stmt_regs(stmt: &Stmt, regs: &mut Vec<u32>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => collect_expr_regs(e, regs),
        Stmt::Declare(_, init) => {
            if let Some(e) = init {
                collect_expr_regs(e, regs);
            }
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::Comment(_)
        | Stmt::ClassDecl(_) => {}
        Stmt::If(cond, then, els) => {
            collect_expr_regs(cond, regs);
            for stmt in then.iter().chain(els) {
                collect_stmt_regs(stmt, regs);
            }
        }
        Stmt::While(cond, body) => {
            collect_expr_regs(cond, regs);
            for stmt in body {
                collect_stmt_regs(stmt, regs);
            }
        }
        Stmt::DoWhile(body, cond) => {
            collect_expr_regs(cond, regs);
            for stmt in body {
                collect_stmt_regs(stmt, regs);
            }
        }
        Stmt::ForOf(_, iter, body) => {
            collect_expr_regs(iter, regs);
            for stmt in body {
                collect_stmt_regs(stmt, regs);
            }
        }
        Stmt::Try(body, catches) => {
            for stmt in body {
                collect_stmt_regs(stmt, regs);
            }
            for (_, handler) in catches {
                for stmt in handler {
                    collect_stmt_regs(stmt, regs);
                }
            }
        }
        Stmt::TryFinally(body, finalizer) => {
            for stmt in body.iter().chain(finalizer) {
                collect_stmt_regs(stmt, regs);
            }
        }
    }
}

fn collect_expr_regs(expr: &Expr, regs: &mut Vec<u32>) {
    match expr {
        Expr::Object(fields) => {
            for (_, value) in fields {
                collect_expr_regs(value, regs);
            }
        }
        Expr::Array(elements) => {
            for value in elements {
                collect_expr_regs(value, regs);
            }
        }
        Expr::Reg(reg) => regs.push(*reg),
        Expr::Bin(_, left, right) | Expr::Assign(left, right) | Expr::Index(left, right) => {
            collect_expr_regs(left, regs);
            collect_expr_regs(right, regs);
        }
        Expr::Un(_, inner)
        | Expr::Spread(inner)
        | Expr::Update(_, inner, _)
        | Expr::Await(inner) => collect_expr_regs(inner, regs),
        Expr::Cond(cond, yes, no) => {
            collect_expr_regs(cond, regs);
            collect_expr_regs(yes, regs);
            collect_expr_regs(no, regs);
        }
        Expr::Member(object, _) => collect_expr_regs(object, regs),
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            collect_expr_regs(callee, regs);
            for arg in args {
                collect_expr_regs(arg, regs);
            }
            if let Some(this_arg) = this_arg {
                collect_expr_regs(this_arg, regs);
            }
        }
        Expr::Arg(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Special(_)
        | Expr::Name(_)
        | Expr::Closure { .. }
        | Expr::LexVar(_, _)
        | Expr::ArrayLit(_)
        | Expr::ObjectLit(_)
        | Expr::ClassLit(_)
        | Expr::TemplateLit(_) => {}
    }
}

fn binary_precedence(op: &str) -> u8 {
    match op {
        "," => 1,
        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "**=" | "&&=" | "||=" | "??=" => 2,
        "??" => 4,
        "||" => 5,
        "&&" => 6,
        "|" => 7,
        "^" => 8,
        "&" => 9,
        "==" | "!=" | "===" | "!==" => 10,
        "<" | ">" | "<=" | ">=" | "in" | "instanceof" => 11,
        "<<" | ">>" | ">>>" => 12,
        "+" | "-" => 13,
        "*" | "/" | "%" => 14,
        "**" => 15,
        _ => 10,
    }
}

fn expr_precedence(e: &Expr) -> u8 {
    match e {
        Expr::Assign(_, _) => 2,
        Expr::Cond(_, _, _) => 3,
        Expr::Bin(op, _, _) => binary_precedence(op),
        Expr::Un(_, _) | Expr::Await(_) => 14,
        Expr::Update(_, _, _) => 16,
        Expr::Member(_, _) | Expr::Index(_, _) | Expr::Call { .. } => 18,
        Expr::Spread(_) | Expr::Closure { .. } => 2,
        _ => 20,
    }
}

fn consume_string_literal(
    pending_key: &mut Option<String>,
    value: String,
) -> Option<(String, String)> {
    if let Some(key) = pending_key.take() {
        Some((key, value))
    } else {
        *pending_key = Some(value);
        None
    }
}

fn method_member(key: &str, rendered: &str) -> String {
    match rendered.find('(') {
        Some(paren) => {
            let prefix = &rendered[..paren];
            let async_prefix = if prefix.starts_with("async ") {
                "async "
            } else {
                ""
            };
            let generator = if prefix.trim_start_matches("async ").starts_with('*') {
                "*"
            } else {
                ""
            };
            format!("{async_prefix}{generator}{key}{}", &rendered[paren..])
        }
        None => format!("{key} {rendered}"),
    }
}

fn format_literal_container(is_class: bool, members: &[String]) -> String {
    if is_class {
        if members.is_empty() {
            "class {}".into()
        } else {
            format!("class {{ {} }}", members.join("; "))
        }
    } else if members.is_empty() {
        "{}".into()
    } else {
        format!("{{ {} }}", members.join(", "))
    }
}

fn reindent_multiline(text: &str, depth: usize) -> String {
    if depth == 0 || !text.contains('\n') {
        return text.to_string();
    }
    let prefix = indent(depth);
    let mut lines = text.split_inclusive('\n');
    let mut out = lines.next().unwrap_or_default().to_string();
    for line in lines {
        out.push_str(&prefix);
        out.push_str(line);
    }
    out
}

fn needs_literal_receiver_parens(e: &Expr) -> bool {
    matches!(e, Expr::Int(_) | Expr::Float(_))
}

/// 带节点预算的结构相等比较：超预算按不相等处理（渲染退化为
/// 显式接收者形式，不影响正确性）
pub(crate) fn expr_eq_bounded(a: &Expr, b: &Expr, budget: &mut i32) -> bool {
    if *budget <= 0 {
        return false;
    }
    *budget -= 1;
    match (a, b) {
        (Expr::Reg(x), Expr::Reg(y)) => x == y,
        (Expr::Arg(x), Expr::Arg(y)) => x == y,
        (Expr::Int(x), Expr::Int(y)) => x == y,
        (Expr::Float(x), Expr::Float(y)) => x == y,
        (Expr::Str(x), Expr::Str(y)) => x == y,
        (Expr::Bool(x), Expr::Bool(y)) => x == y,
        (Expr::Special(x), Expr::Special(y)) => x == y,
        (Expr::Name(x), Expr::Name(y)) => x == y,
        (Expr::Bin(op1, a1, b1), Expr::Bin(op2, a2, b2)) => {
            op1 == op2 && expr_eq_bounded(a1, a2, budget) && expr_eq_bounded(b1, b2, budget)
        }
        (Expr::Un(op1, a1), Expr::Un(op2, a2)) => op1 == op2 && expr_eq_bounded(a1, a2, budget),
        (Expr::Update(op1, a1, p1), Expr::Update(op2, a2, p2)) => {
            op1 == op2 && p1 == p2 && expr_eq_bounded(a1, a2, budget)
        }
        (Expr::Assign(t1, v1), Expr::Assign(t2, v2)) => {
            expr_eq_bounded(t1, t2, budget) && expr_eq_bounded(v1, v2, budget)
        }
        (Expr::Cond(c1, a1, b1), Expr::Cond(c2, a2, b2)) => {
            expr_eq_bounded(c1, c2, budget)
                && expr_eq_bounded(a1, a2, budget)
                && expr_eq_bounded(b1, b2, budget)
        }
        (Expr::Member(o1, n1), Expr::Member(o2, n2)) => n1 == n2 && expr_eq_bounded(o1, o2, budget),
        (Expr::Index(o1, k1), Expr::Index(o2, k2)) => {
            expr_eq_bounded(o1, o2, budget) && expr_eq_bounded(k1, k2, budget)
        }
        (
            Expr::Call {
                callee: c1,
                args: g1,
                new: n1,
                this_arg: t1,
            },
            Expr::Call {
                callee: c2,
                args: g2,
                new: n2,
                this_arg: t2,
            },
        ) => {
            n1 == n2
                && expr_eq_bounded(c1, c2, budget)
                && g1.len() == g2.len()
                && g1
                    .iter()
                    .zip(g2)
                    .all(|(x, y)| expr_eq_bounded(x, y, budget))
                && match (t1, t2) {
                    (Some(x), Some(y)) => expr_eq_bounded(x, y, budget),
                    (None, None) => true,
                    _ => false,
                }
        }
        (Expr::Closure { method_off: x }, Expr::Closure { method_off: y }) => x == y,
        (Expr::LexVar(d1, s1), Expr::LexVar(d2, s2)) => d1 == d2 && s1 == s2,
        (Expr::ArrayLit(x), Expr::ArrayLit(y)) => x == y,
        (Expr::ObjectLit(x), Expr::ObjectLit(y)) => x == y,
        (Expr::Object(x), Expr::Object(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((key_a, val_a), (key_b, val_b))| {
                    key_a == key_b && expr_eq_bounded(val_a, val_b, budget)
                })
        }
        (Expr::Array(x), Expr::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| expr_eq_bounded(a, b, budget))
        }
        (Expr::ClassLit(x), Expr::ClassLit(y)) => x == y,
        (Expr::TemplateLit(x), Expr::TemplateLit(y)) => x == y,
        (Expr::Spread(a), Expr::Spread(b)) => expr_eq_bounded(a, b, budget),
        (Expr::Await(a), Expr::Await(b)) => expr_eq_bounded(a, b, budget),
        _ => false,
    }
}

impl Renderer<'_> {
    /// 函数、闭包、类成员和模块顶层共用的声明规划；不向子块移动声明。
    fn render_body(&self, stmts: &[Stmt], is_concurrent: bool, depth: usize, out: &mut String) {
        // taskpool 指令须在局部声明前，才能重新编译为 notifyconcurrentresult 协议。
        if is_concurrent {
            out.push_str(&indent(depth));
            out.push_str("\"use concurrent\";\n");
        }
        let (regs, initializers) = local_declarations(stmts, &self.names);
        write_local_declaration(&regs, &self.names, depth, out);
        for (index, stmt) in stmts.iter().enumerate() {
            if initializers.contains(&index)
                && let Stmt::Expr(Expr::Assign(target, value)) = stmt
                && let Expr::Reg(reg) = target.as_ref()
            {
                self.render_local(&self.names.reg(*reg), Some(value), depth, out);
            } else {
                self.render_stmt(stmt, depth, out);
            }
        }
    }

    fn render_local(&self, name: &str, init: Option<&Expr>, depth: usize, out: &mut String) {
        let previous_depth = self.indent_depth.replace(depth);
        out.push_str(&indent(depth));
        out.push_str(&format!("let {name}"));
        if let Some(value) = init {
            out.push_str(" = ");
            self.write_expr(value, out);
        }
        out.push_str(";\n");
        self.indent_depth.set(previous_depth);
    }

    fn render_stmt(&self, s: &Stmt, depth: usize, out: &mut String) {
        let previous_depth = self.indent_depth.replace(depth);
        match s {
            Stmt::Expr(e) => {
                if let Expr::Assign(target, value) = e
                    && let (Expr::Reg(reg), Expr::Arg(arg)) = (target.as_ref(), value.as_ref())
                    && self.names.parameter_copies.get(reg) == Some(arg)
                {
                    self.indent_depth.set(previous_depth);
                    return;
                }
                out.push_str(&indent(depth));
                self.write_expr(e, out);
                out.push_str(";\n");
            }
            Stmt::Declare(name, init) => {
                self.render_local(name, init.as_ref(), depth, out);
            }
            Stmt::Return(Some(e)) => {
                out.push_str(&indent(depth));
                out.push_str("return ");
                self.write_expr(e, out);
                out.push_str(";\n");
            }
            Stmt::Return(None) => {
                out.push_str(&indent(depth));
                out.push_str("return;\n");
            }
            Stmt::Throw(e) => {
                out.push_str(&indent(depth));
                out.push_str("throw ");
                self.write_expr(e, out);
                out.push_str(";\n");
            }
            Stmt::If(cond, then, els) => {
                out.push_str(&indent(depth));
                out.push_str("if (");
                self.write_expr(cond, out);
                out.push_str(") {\n");
                for s in then {
                    self.render_stmt(s, depth + 1, out);
                }
                if !els.is_empty() {
                    if els.len() == 1 && matches!(els.first(), Some(Stmt::If(..))) {
                        out.push_str(&format!("{}}} else ", indent(depth)));
                        self.render_stmt(&els[0], depth, out);
                        self.indent_depth.set(previous_depth);
                        return;
                    } else {
                        out.push_str(&format!("{}}} else {{\n", indent(depth)));
                        for s in els {
                            self.render_stmt(s, depth + 1, out);
                        }
                    }
                }
                out.push_str(&format!("{}}}\n", indent(depth)));
            }
            Stmt::While(cond, body) => {
                out.push_str(&indent(depth));
                out.push_str("while (");
                self.write_expr(cond, out);
                out.push_str(") {\n");
                for s in body {
                    self.render_stmt(s, depth + 1, out);
                }
                out.push_str(&format!("{}}}\n", indent(depth)));
            }
            Stmt::DoWhile(body, cond) => {
                out.push_str(&indent(depth));
                out.push_str("do {\n");
                for s in body {
                    self.render_stmt(s, depth + 1, out);
                }
                out.push_str(&indent(depth));
                out.push_str("} while (");
                self.write_expr(cond, out);
                out.push_str(");\n");
            }
            Stmt::ForOf(var, iter, body) => {
                out.push_str(&indent(depth));
                out.push_str(&format!("for (let {var} of "));
                self.write_expr(iter, out);
                out.push_str(") {\n");
                for s in body {
                    self.render_stmt(s, depth + 1, out);
                }
                out.push_str(&format!("{}}}\n", indent(depth)));
            }
            Stmt::Try(body, catches) => {
                out.push_str(&indent(depth));
                out.push_str("try {\n");
                for s in body {
                    self.render_stmt(s, depth + 1, out);
                }
                for (name, handler) in catches {
                    let binder = name.clone().unwrap_or_else(|| "e".into());
                    out.push_str(&format!("{}}} catch ({binder}) {{\n", indent(depth)));
                    for s in handler {
                        self.render_stmt(s, depth + 1, out);
                    }
                }
                out.push_str(&format!("{}}}\n", indent(depth)));
            }
            Stmt::TryFinally(body, finalizer) => {
                out.push_str(&format!("{}try {{\n", indent(depth)));
                for stmt in body {
                    self.render_stmt(stmt, depth + 1, out);
                }
                out.push_str(&format!("{}}} finally {{\n", indent(depth)));
                for stmt in finalizer {
                    self.render_stmt(stmt, depth + 1, out);
                }
                out.push_str(&format!("{}}}\n", indent(depth)));
            }
            Stmt::ClassDecl(constructor) => {
                let recovery = self.nested.borrow().recovery.clone();
                let text = recovery.and_then(|recovery| {
                    recovery.render_class(self.file, *constructor, &self.nested)
                });
                if let Some(text) = text {
                    for line in text.lines() {
                        out.push_str(&indent(depth));
                        out.push_str(line);
                        out.push('\n');
                    }
                } else {
                    out.push_str(&format!(
                        "{}/* unresolved class @ {constructor:#x} */\n",
                        indent(depth)
                    ));
                }
            }
            Stmt::Break => out.push_str(&format!("{}break;\n", indent(depth))),
            Stmt::Continue => out.push_str(&format!("{}continue;\n", indent(depth))),
            Stmt::Comment(text) => {
                out.push_str(&indent(depth));
                out.push_str(&format!("/* {} */\n", text.replace('\n', " ")));
            }
        }
        self.indent_depth.set(previous_depth);
    }

    /// 表达式写入共享缓冲区：深链场景下 format! 每层都会拷贝子串，
    /// 总成本 O(n²)；追加写入为 O(n)
    fn write_expr(&self, e: &Expr, out: &mut String) {
        self.write_expr_prec(e, 0, false, None, out);
    }

    fn write_child(
        &self,
        e: &Expr,
        parent_prec: u8,
        equal_wrap: bool,
        parent_op: Option<&str>,
        out: &mut String,
    ) {
        let child_prec = expr_precedence(e);
        let mixed_nullish = matches!(
            (parent_op, e),
            (Some("??"), Expr::Bin("&&" | "||", _, _)) | (Some("&&" | "||"), Expr::Bin("??", _, _))
        );
        let wrap =
            child_prec < parent_prec || (equal_wrap && child_prec == parent_prec) || mixed_nullish;
        if wrap {
            out.push('(');
        }
        self.write_expr_prec(e, parent_prec, equal_wrap, parent_op, out);
        if wrap {
            out.push(')');
        }
    }

    fn write_expr_prec(
        &self,
        e: &Expr,
        _parent_prec: u8,
        _equal_wrap: bool,
        _parent_op: Option<&str>,
        out: &mut String,
    ) {
        match e {
            Expr::Reg(n) => out.push_str(&self.names.reg(*n)),
            Expr::Arg(n) => {
                if self.modern_abi && *n < 3 {
                    out.push_str(match n {
                        0 => "currentFunction",
                        1 => "new.target",
                        _ => "this",
                    });
                } else if self.modern_abi {
                    out.push_str(&self.names.arg(*n, true));
                } else if *n == 0 && self.this_arg0 {
                    out.push_str("this");
                } else {
                    out.push_str(&self.names.arg(*n, false));
                }
            }
            Expr::Int(v) => out.push_str(&format!("{v}")),
            Expr::Float(v) => {
                if v.is_finite() {
                    out.push_str(&format!("{v}"));
                } else if v.is_nan() {
                    out.push_str("NaN");
                } else if *v > 0.0 {
                    out.push_str("Infinity");
                } else {
                    out.push_str("-Infinity");
                }
            }
            Expr::Str(s) => out.push_str(&format!("{s:?}")),
            Expr::Bool(b) => out.push_str(&format!("{b}")),
            Expr::Special(s) => out.push_str(s),
            Expr::Name(n) => out.push_str(n),
            Expr::Bin(op, a, b) => {
                let prec = binary_precedence(op);
                self.write_child(a, prec, *op == "**", Some(op), out);
                out.push(' ');
                out.push_str(op);
                out.push(' ');
                self.write_child(b, prec, *op != "**", Some(op), out);
            }
            Expr::Un(op, a) => {
                if matches!(*op, "typeof" | "delete" | "void") {
                    out.push_str(op);
                    out.push(' ');
                    self.write_child(a, 14, false, Some(op), out);
                } else {
                    out.push_str(op);
                    self.write_child(a, 15, true, Some(op), out);
                }
            }
            Expr::Await(inner) => {
                out.push_str("await ");
                self.write_child(inner, 16, false, Some("await"), out);
            }
            Expr::Update(op, a, post) => {
                if *post {
                    self.write_expr(a, out);
                    out.push_str(op);
                } else {
                    out.push_str(op);
                    self.write_expr(a, out);
                }
            }
            Expr::Assign(t, v) => {
                self.write_child(t, 2, true, Some("="), out);
                out.push_str(" = ");
                self.write_child(v, 2, false, Some("="), out);
            }
            Expr::Cond(c, a, b) => {
                self.write_child(c, 3, false, Some("?"), out);
                out.push_str(" ? ");
                self.write_child(a, 2, false, Some("?"), out);
                out.push_str(" : ");
                self.write_child(b, 2, false, Some("?"), out);
            }
            Expr::Member(o, name) => {
                let needs_brackets = name.is_empty()
                    || !name
                        .chars()
                        .next()
                        .map(|c| c.is_alphabetic() || c == '_' || c == '$')
                        .unwrap_or(false);
                if needs_brackets {
                    if needs_literal_receiver_parens(o) {
                        out.push('(');
                        self.write_expr(o, out);
                        out.push(')');
                    } else {
                        self.write_child(o, 18, false, Some("member"), out);
                    }
                    out.push_str(&format!("[{name:?}]"));
                } else {
                    if needs_literal_receiver_parens(o) {
                        out.push('(');
                        self.write_expr(o, out);
                        out.push(')');
                    } else {
                        self.write_child(o, 18, false, Some("member"), out);
                    }
                    out.push('.');
                    out.push_str(name);
                }
            }
            Expr::Index(o, k) => {
                if needs_literal_receiver_parens(o) {
                    out.push('(');
                    self.write_expr(o, out);
                    out.push(')');
                } else {
                    self.write_child(o, 18, false, Some("member"), out);
                }
                out.push('[');
                self.write_expr(k, out);
                out.push(']');
            }
            Expr::Call {
                callee,
                args,
                new,
                this_arg,
            } => {
                // this 调用融合：obj.m(args) 当 callee 是 obj.m 且 this == obj。
                // 有界结构比较：无界 PartialEq 在 fold 后的大全等树上
                // 每个调用点全树遍历，整体退化为 O(n²)
                // this 为 undefined/null 字面量：普通函数调用形式
                let drop_this = matches!(
                    this_arg.as_deref(),
                    Some(Expr::Special("undefined") | Expr::Special("null"))
                );
                if !*new
                    && !drop_this
                    && this_arg.is_some()
                    && let (Some(t), Expr::Member(o, m)) = (this_arg, callee.as_ref())
                {
                    let mut budget = 512i32;
                    if expr_eq_bounded(t, o, &mut budget) {
                        self.write_expr(o, out);
                        out.push('.');
                        out.push_str(m);
                        out.push('(');
                        for (i, a) in args.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            self.write_expr(a, out);
                        }
                        out.push(')');
                        return;
                    }
                }
                let explicit_receiver = this_arg.as_ref().filter(|_| !drop_this && !*new);
                if let Some(receiver) = explicit_receiver {
                    // VM 调用不读取函数自身的 call 属性；该属性可以被用户重写。
                    out.push_str("globalThis.Reflect.apply(");
                    self.write_expr(callee, out);
                    out.push_str(", ");
                    self.write_expr(receiver, out);
                    out.push_str(", [");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        self.write_expr(arg, out);
                    }
                    out.push_str("])");
                    return;
                }
                if *new {
                    out.push_str("new ");
                }
                if *new && matches!(callee.as_ref(), Expr::Call { .. }) {
                    out.push('(');
                    self.write_expr(callee, out);
                    out.push(')');
                } else {
                    self.write_child(callee, 18, false, Some("call"), out);
                }
                out.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.write_expr(a, out);
                }
                out.push(')');
            }
            Expr::Closure { method_off } => {
                let text = if self.depth < NestedCache::MAX_DEPTH {
                    crate::decompile::render_nested_function(
                        self.file,
                        *method_off,
                        self.depth + 1,
                        crate::decompile::NestedStyle::Arrow,
                        &self.nested.clone(),
                    )
                } else {
                    None
                };
                match text {
                    Some(t) if self.nested.borrow_mut().embed_closure(t.len()) => {
                        let adjusted = reindent_multiline(&t, self.indent_depth.get());
                        out.push_str(&adjusted);
                    }
                    _ => out.push_str(&format!(
                        "() => {{ throw new Error(\"Unresolved closure @ {method_off:#x}\"); }}"
                    )),
                }
            }
            Expr::LexVar(depth, slot) => out.push_str(&format!("lex{depth}_{slot}")),
            Expr::ArrayLit(off) => out.push_str(&self.render_array_lit(*off)),
            Expr::ObjectLit(off) => out.push_str(&self.render_object_lit(*off, false)),
            Expr::Object(fields) => {
                out.push_str("{ ");
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&object_property_key(key));
                    out.push_str(": ");
                    self.write_child(value, 2, false, None, out);
                }
                out.push_str(" }");
            }
            Expr::Array(elements) => {
                out.push('[');
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    self.write_child(element, 2, false, None, out);
                }
                out.push(']');
            }
            Expr::ClassLit(off) => out.push_str(&self.render_object_lit(*off, true)),
            Expr::TemplateLit(t) => out.push_str(t),
            Expr::Spread(e) => {
                out.push_str("...");
                self.write_child(e, 2, false, Some("..."), out);
            }
        }
    }

    /// 对象字面量方法体展开；缓存允许同一方法在多个表达式中复用。
    fn nested_member(&self, method_off: u32) -> Option<String> {
        if self.depth >= NestedCache::MAX_DEPTH {
            return None;
        }
        let text = crate::decompile::render_nested_function(
            self.file,
            method_off,
            self.depth + 1,
            crate::decompile::NestedStyle::Method,
            &self.nested.clone(),
        )?;
        self.nested
            .borrow_mut()
            .embed_closure(text.len())
            .then_some(text)
    }

    /// 数组字面量渲染（记忆化 + 环守卫）
    fn render_array_lit(&self, off: u32) -> String {
        if let Some(hit) = self.lit_cache.borrow().get(&off) {
            return hit
                .clone()
                .unwrap_or_else(|| format!("/*cyclic array@{off:#x}*/"));
        }
        self.lit_cache.borrow_mut().insert(off, None);
        let lits = match self.file.read_literal_array_truncated(off) {
            Ok(l) => l,
            Err(e) => {
                self.lit_cache.borrow_mut().remove(&off);
                return format!("/*array literal@{off:#x}: {e}*/[]");
            }
        };
        let items: Vec<String> = lits
            .values
            .iter()
            .map(|v| self.render_literal_value(v))
            .collect();
        let out = if lits.total > lits.values.len() {
            format!(
                "[{} /* +{} more */]",
                items.join(", "),
                lits.total - lits.values.len()
            )
        } else {
            format!("[{}]", items.join(", "))
        };
        self.lit_cache.borrow_mut().insert(off, Some(out.clone()));
        out
    }

    /// 对象/类字面量渲染：STRING 键 + 值交替（记忆化 + 环守卫）
    fn render_object_lit(&self, off: u32, is_class: bool) -> String {
        if off == 0 && !is_class {
            return "{}".into();
        }
        let key = off.wrapping_mul(2).wrapping_add(u32::from(is_class));
        if let Some(hit) = self.lit_cache.borrow().get(&key) {
            return hit
                .clone()
                .unwrap_or_else(|| format!("{{ /*cyclic literal@{off:#x}*/ }}"));
        }
        self.lit_cache.borrow_mut().insert(key, None);
        let lits = match self.file.read_literal_array(off) {
            Ok(l) => l,
            Err(_) => {
                return if is_class {
                    format!("class {{ /*literal@{off:#x}*/ }}")
                } else {
                    format!("{{ /*literal@{off:#x}*/ }}")
                };
            }
        };
        let mut members = Vec::new();
        let mut pending_key: Option<String> = None;
        for v in lits.iter() {
            match v {
                LiteralValue::String(off) => {
                    if let Ok(value) = self.file.string_at(*off)
                        && let Some((property, value)) =
                            consume_string_literal(&mut pending_key, value)
                    {
                        members.push(format!("{}: {value:?}", object_property_key(&property)));
                    }
                }
                LiteralValue::Method(m)
                | LiteralValue::GeneratorMethod(m)
                | LiteralValue::AsyncGeneratorMethod(m) => {
                    if let Some(key) = pending_key.take() {
                        let body = self.nested_member(*m);
                        members.push(match body {
                            Some(text) => method_member(&object_property_key(&key), &text),
                            None => {
                                format!("{}() {{ /*method@{m:#x}*/ }}", object_property_key(&key))
                            }
                        });
                    } else {
                        members.push(format!("/*method@{m:#x}*/"));
                    }
                }
                LiteralValue::Getter(g) => {
                    if let Some(key) = pending_key.take() {
                        let body = self.nested_member(*g);
                        members.push(match body {
                            Some(text) => {
                                format!("get {}", method_member(&object_property_key(&key), &text))
                            }
                            None => {
                                format!("get {}() {{ /*@{g:#x}*/ }}", object_property_key(&key))
                            }
                        });
                    }
                }
                LiteralValue::Setter(sv) => {
                    if let Some(key) = pending_key.take() {
                        let body = self.nested_member(*sv);
                        members.push(match body {
                            Some(text) => {
                                format!("set {}", method_member(&object_property_key(&key), &text))
                            }
                            None => {
                                format!("set {}() {{ /*@{sv:#x}*/ }}", object_property_key(&key))
                            }
                        });
                    }
                }
                LiteralValue::MethodAffiliate(_) => {}
                other => {
                    if let Some(key) = pending_key.take() {
                        members.push(format!(
                            "{}: {}",
                            object_property_key(&key),
                            self.render_literal_value(other)
                        ));
                    }
                }
            }
        }
        let out = format_literal_container(is_class, &members);
        self.lit_cache.borrow_mut().insert(key, Some(out.clone()));
        out
    }

    /// 单个 literal 值渲染
    pub fn render_literal_value(&self, v: &LiteralValue) -> String {
        match v {
            LiteralValue::Bool(b) => b.to_string(),
            LiteralValue::Int8(v) => format!("{v}"),
            LiteralValue::Int16(v) => format!("{v}"),
            LiteralValue::Int32(v) => format!("{v}"),
            LiteralValue::Uint32(v) => format!("{v}"),
            LiteralValue::Uint64(v) => format!("{v}"),
            LiteralValue::Float(v) => format!("{v}"),
            LiteralValue::Double(v) => format!("{v}"),
            LiteralValue::String(off) => {
                format!("{:?}", self.file.string_at(*off).unwrap_or_default())
            }
            LiteralValue::Method(off) => format!("/*method@{off:#x}*/"),
            LiteralValue::GeneratorMethod(off) => format!("/*generator@{off:#x}*/"),
            LiteralValue::AsyncGeneratorMethod(off) => format!("/*asyncgen@{off:#x}*/"),
            LiteralValue::Accessor(off) => format!("/*accessor@{off:#x}*/"),
            LiteralValue::MethodAffiliate(v) => format!("{v}"),
            LiteralValue::ArrayU1(a) => format!("{a:?}"),
            LiteralValue::ArrayU8(a) => format!("{a:?}"),
            LiteralValue::ArrayI8(a) => format!("{a:?}"),
            LiteralValue::ArrayU16(a) => format!("{a:?}"),
            LiteralValue::ArrayI16(a) => format!("{a:?}"),
            LiteralValue::ArrayU32(a) => format!("{a:?}"),
            LiteralValue::ArrayI32(a) => format!("{a:?}"),
            LiteralValue::ArrayU64(a) => format!("{a:?}"),
            LiteralValue::ArrayI64(a) => format!("{a:?}"),
            LiteralValue::ArrayF32(a) => format!("{a:?}"),
            LiteralValue::ArrayF64(a) => format!("{a:?}"),
            LiteralValue::ArrayString(a) => {
                let s: Vec<String> = a
                    .iter()
                    .map(|o| format!("{:?}", self.file.string_at(*o).unwrap_or_default()))
                    .collect();
                format!("[{}]", s.join(", "))
            }
            LiteralValue::LiteralBufferIndex(v) => format!("{v}"),
            LiteralValue::LiteralArray(off) => self.render_array_lit(*off),
            LiteralValue::BuiltinTypeIndex(v) => format!("{v}"),
            LiteralValue::Getter(off) => format!("/*getter@{off:#x}*/"),
            LiteralValue::Setter(off) => format!("/*setter@{off:#x}*/"),
            LiteralValue::EtsImplements(off) => format!("implements@{off:#x}"),
            LiteralValue::Null => "null".into(),
        }
    }
}

/// 渲染超大方法：完整反汇编 + 伪代码截断提示
pub fn render_large_method(
    file: &AbcFile,
    m: &MethodInfo,
    code: &CodeInfo,
    bytes: &[u8],
) -> String {
    let legacy = file.header.kind == crate::file::HeaderKind::Legacy;
    let ctx = crate::disasm::DisasmCtx {
        file,
        method_off: m.offset,
    };
    let mut out = format!(
        "// method {} @ {:#x}: {} bytes of bytecode (large method, full disasm)\n",
        m.name,
        m.offset,
        bytes.len()
    );
    match crate::disasm::decode_with(bytes, legacy) {
        Ok(ins) => {
            out.push_str(&format!(
                "// {} instructions, vregs={}, args={}\n",
                ins.len(),
                code.num_vregs,
                code.num_args
            ));
            for i in ins.iter().take(4000) {
                out.push_str(&format!("  {}\n", crate::disasm::render_ins(&ctx, i)));
            }
            if ins.len() > 4000 {
                out.push_str(&format!("  // … {} more instructions\n", ins.len() - 4000));
            }
        }
        Err(e) => out.push_str(&format!("// decode failed: {e}\n")),
    }
    out
}

/// 渲染模块 import/export 头部
/// 导出名清理：`*default*` / `*name*` 修饰 → 还原
fn clean_export_name(name: &str) -> String {
    let trimmed = name.trim_matches('*');
    if trimmed.is_empty() {
        name.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn render_module_header(m: &crate::file::ModuleRecord) -> String {
    let mut out = String::new();
    for r in &m.regular_imports {
        let module = m
            .requests
            .get(r.module_idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("?");
        if r.local_name == r.import_name {
            out.push_str(&format!(
                "import {{ {} }} from {:?};\n",
                r.local_name, module
            ));
        } else {
            out.push_str(&format!(
                "import {{ {} as {} }} from {:?};\n",
                r.import_name, r.local_name, module
            ));
        }
    }
    for n in &m.namespace_imports {
        let module = m
            .requests
            .get(n.module_idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("?");
        out.push_str(&format!(
            "import * as {} from {:?};\n",
            n.local_name, module
        ));
    }
    for e in &m.indirect_exports {
        let module = m
            .requests
            .get(e.module_idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("?");
        if e.export_name == e.import_name {
            out.push_str(&format!(
                "export {{ {} }} from {:?};\n",
                clean_export_name(&e.export_name),
                module
            ));
        } else {
            out.push_str(&format!(
                "export {{ {} as {} }} from {:?};\n",
                clean_export_name(&e.import_name),
                clean_export_name(&e.export_name),
                module
            ));
        }
    }
    for &idx in &m.star_exports {
        let module = m
            .requests
            .get(idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("?");
        out.push_str(&format!("export * from {:?};\n", module));
    }
    if !m.local_exports.is_empty() {
        let names: Vec<String> = m
            .local_exports
            .iter()
            .map(|e| {
                if e.local_name == e.export_name {
                    clean_export_name(&e.local_name)
                } else {
                    format!(
                        "{} as {}",
                        clean_export_name(&e.local_name),
                        clean_export_name(&e.export_name)
                    )
                }
            })
            .collect();
        out.push_str(&format!("export {{ {} }};\n", names.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompile::lift::LiftedBody;
    use crate::file::{FileHeader, FunctionKind, HeaderKind};

    fn renderer(modern_abi: bool) -> Renderer<'static> {
        let file = Box::leak(Box::new(AbcFile {
            data: Vec::new(),
            header: FileHeader {
                version: [9, 0, 0, 0],
                file_size: 0,
                foreign_off: 0,
                foreign_size: 0,
                num_classes: 0,
                class_idx_off: 0,
                num_lnps: 0,
                lnp_idx_off: 0,
                num_literalarrays: 0,
                literalarray_idx_off: 0,
                num_index_regions: 0,
                index_section_off: 0,
                kind: if modern_abi {
                    HeaderKind::Modern
                } else {
                    HeaderKind::Legacy
                },
            },
            class_index: Vec::new(),
            index_headers: Vec::new(),
            classes: Vec::new(),
        }));
        Renderer {
            file,
            lit_cache: RefCell::new(HashMap::new()),
            nested: std::rc::Rc::new(RefCell::new(NestedCache {
                recovery: None,
                map: HashMap::new(),
                closure_budget: 8,
                closure_bytes: 0,
                member_budget: 8,
                member_bytes: 0,
                closure_emitted_bytes: 0,
            })),
            depth: 0,
            this_arg0: !modern_abi,
            modern_abi,
            indent_depth: Cell::new(0),
            names: NameContext::default(),
        }
    }

    fn expression(r: &Renderer<'_>, e: &Expr) -> String {
        let mut out = String::new();
        r.write_expr(e, &mut out);
        out
    }

    fn assign_reg(reg: u32, value: Expr) -> Stmt {
        Stmt::Expr(Expr::Assign(Box::new(Expr::Reg(reg)), Box::new(value)))
    }

    fn rendered_body(r: &Renderer<'_>, stmts: &[Stmt]) -> String {
        let mut out = String::new();
        r.render_body(stmts, false, 1, &mut out);
        out
    }

    #[test]
    fn register_initializers_keep_evaluation_order_and_subsequent_writes() {
        let r = renderer(true);
        let output = rendered_body(
            &r,
            &[
                assign_reg(
                    1,
                    Expr::Member(Box::new(Expr::Name("Config".into())), "value".into()),
                ),
                assign_reg(0, Expr::Name("DownloadSceneType".into())),
                assign_reg(
                    1,
                    Expr::Call {
                        callee: Box::new(Expr::Name("nextValue".into())),
                        args: Vec::new(),
                        new: false,
                        this_arg: None,
                    },
                ),
                Stmt::Return(Some(Expr::Reg(1))),
            ],
        );
        assert_eq!(
            output,
            "    let v1 = Config.value;\n    let v0 = DownloadSceneType;\n    v1 = nextValue();\n    return v1;\n"
        );
    }

    #[test]
    fn register_reads_before_assignment_and_self_references_keep_hoisted_declarations() {
        let r = renderer(true);
        let output = rendered_body(
            &r,
            &[
                assign_reg(1, Expr::Reg(0)),
                assign_reg(0, Expr::Int(1)),
                assign_reg(
                    2,
                    Expr::Bin("??", Box::new(Expr::Reg(2)), Box::new(Expr::Int(0))),
                ),
                assign_reg(3, Expr::Object(vec![("old".into(), Expr::Reg(3))])),
                assign_reg(4, Expr::Array(vec![Expr::Reg(4)])),
            ],
        );
        assert!(
            output.starts_with("    let v0, v2, v3, v4;\n    let v1 = v0;\n"),
            "{output}"
        );
        for reg in [0, 2, 3, 4] {
            assert!(!output.contains(&format!("let v{reg} =")), "{output}");
        }
    }

    #[test]
    fn register_declarations_stay_outside_branches_loops_and_exception_handlers() {
        let r = renderer(true);
        let inner = vec![assign_reg(0, Expr::Int(1))];
        for control in [
            Stmt::If(Expr::Arg(3), inner.clone(), Vec::new()),
            Stmt::While(Expr::Arg(3), inner.clone()),
            Stmt::DoWhile(inner.clone(), Expr::Arg(3)),
            Stmt::ForOf("item".into(), Expr::Arg(3), inner.clone()),
            Stmt::Try(Vec::new(), vec![(Some("error".into()), inner.clone())]),
            Stmt::TryFinally(Vec::new(), inner),
        ] {
            let output = rendered_body(
                &r,
                &[
                    control,
                    assign_reg(0, Expr::Int(2)),
                    Stmt::Return(Some(Expr::Reg(0))),
                ],
            );
            assert!(output.starts_with("    let v0;\n"), "{output}");
            assert!(!output.contains("let v0 ="), "{output}");
        }
    }

    #[test]
    fn register_initializers_preserve_debug_names_and_parameter_aliases() {
        let mut r = renderer(true);
        r.names.args.insert(3, "input".into());
        r.names.regs.insert(0, "input".into());
        r.names.parameter_copies.insert(0, 3);
        r.names.regs.insert(1, "scene".into());
        let output = rendered_body(
            &r,
            &[
                assign_reg(0, Expr::Arg(3)),
                assign_reg(1, Expr::Reg(0)),
                assign_reg(0, Expr::Int(2)),
                Stmt::Return(Some(Expr::Reg(1))),
            ],
        );
        assert_eq!(
            output,
            "    let scene = input;\n    input = 2;\n    return scene;\n"
        );
    }

    #[test]
    fn all_body_renderers_merge_register_initializers() {
        let r = renderer(true);
        let method = MethodInfo {
            offset: 0,
            class_off: 0,
            proto_off: 0,
            name: "readScene".into(),
            header_index: 0,
            kind: FunctionKind::Function,
            code: None,
            debug_info_off: 0,
            foreign: false,
        };
        let code = CodeInfo {
            offset: 0,
            num_vregs: 1,
            num_args: 3,
            code_size: 0,
            ins_off: 0,
            try_blocks: Vec::new(),
        };
        let class = ClassInfo {
            offset: 0,
            name: "Probe".into(),
            access_flags: 0,
            source_file: None,
            fields: Vec::new(),
            methods: Vec::new(),
            module_record_idx: None,
        };
        let body = LiftedBody {
            stmts: vec![
                assign_reg(0, Expr::Name("DownloadSceneType".into())),
                Stmt::Return(Some(Expr::Reg(0))),
            ],
            is_async: false,
            is_concurrent: false,
        };
        let outputs = [
            render_method(r.file, &class, &method, &code, &body, &r.nested),
            render_nested(
                r.file,
                &method,
                &code,
                &body,
                0,
                crate::decompile::NestedStyle::Arrow,
                &r.nested,
            ),
            render_nested(
                r.file,
                &method,
                &code,
                &body,
                0,
                crate::decompile::NestedStyle::Method,
                &r.nested,
            ),
            render_top_level(r.file, &body, &r.nested),
        ];
        for output in outputs {
            assert!(output.contains("let v0 = DownloadSceneType;"), "{output}");
            assert!(!output.contains("let v0;"), "{output}");
            assert_eq!(output.matches("DownloadSceneType").count(), 1, "{output}");
        }
    }

    #[test]
    fn captured_arguments_print_as_initialized_declarations() {
        let r = renderer(true);
        let mut body = LiftedBody {
            stmts: vec![
                Stmt::Declare("captured_17af6_0".into(), None),
                Stmt::Declare("captured_17af6_1".into(), None),
                Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Name("captured_17af6_0".into())),
                    Box::new(Expr::Arg(3)),
                )),
                Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Name("captured_17af6_1".into())),
                    Box::new(Expr::Arg(4)),
                )),
                Stmt::Return(Some(Expr::Name("captured_17af6_0".into()))),
            ],
            is_async: false,
            is_concurrent: false,
        };
        crate::decompile::structure::polish(&mut body.stmts);
        let output = render_top_level(r.file, &body, &r.nested);
        assert_eq!(
            output,
            "let captured_17af6_0 = a0;\nlet captured_17af6_1 = a1;\nreturn captured_17af6_0;\n"
        );
    }

    #[test]
    fn expression_precedence_preserves_grouping() {
        let r = renderer(true);
        let mul_add = Expr::Bin(
            "+",
            Box::new(Expr::Name("a".into())),
            Box::new(Expr::Bin(
                "*",
                Box::new(Expr::Name("b".into())),
                Box::new(Expr::Name("c".into())),
            )),
        );
        let add_mul = Expr::Bin(
            "*",
            Box::new(Expr::Bin(
                "+",
                Box::new(Expr::Name("a".into())),
                Box::new(Expr::Name("b".into())),
            )),
            Box::new(Expr::Name("c".into())),
        );
        let pow_unary = Expr::Bin(
            "**",
            Box::new(Expr::Un("-", Box::new(Expr::Name("a".into())))),
            Box::new(Expr::Name("b".into())),
        );
        let nullish = Expr::Bin(
            "??",
            Box::new(Expr::Name("a".into())),
            Box::new(Expr::Bin(
                "||",
                Box::new(Expr::Name("b".into())),
                Box::new(Expr::Name("c".into())),
            )),
        );
        let unary_power = Expr::Un(
            "-",
            Box::new(Expr::Un(
                "~",
                Box::new(Expr::Bin(
                    "**",
                    Box::new(Expr::Name("a".into())),
                    Box::new(Expr::Name("b".into())),
                )),
            )),
        );
        assert_eq!(expression(&r, &mul_add), "a + b * c");
        assert_eq!(expression(&r, &add_mul), "(a + b) * c");
        assert_eq!(expression(&r, &pow_unary), "(-a) ** b");
        assert_eq!(expression(&r, &nullish), "a ?? (b || c)");
        assert_eq!(expression(&r, &unary_power), "-(~(a ** b))");
    }

    #[test]
    fn parsed_array_literals_print_spreads_and_compare_structurally() {
        let r = renderer(true);
        let array = Expr::Array(vec![
            Expr::Spread(Box::new(Expr::Name("arguments".into()))),
            Expr::Int(1),
        ]);
        let equivalent = Expr::Array(vec![
            Expr::Spread(Box::new(Expr::Name("arguments".into()))),
            Expr::Int(1),
        ]);
        let different = Expr::Array(vec![
            Expr::Spread(Box::new(Expr::Name("arguments".into()))),
            Expr::Int(2),
        ]);

        assert_eq!(expression(&r, &array), "[...arguments, 1]");
        assert!(expr_eq_bounded(&array, &equivalent, &mut 32));
        assert!(!expr_eq_bounded(&array, &different, &mut 32));
        assert!(crate::decompile::lift::has_effect(&array));
    }

    #[test]
    fn modern_implicit_args_and_user_params_are_named_cleanly() {
        let r = renderer(true);
        assert_eq!(expression(&r, &Expr::Arg(0)), "currentFunction");
        assert_eq!(expression(&r, &Expr::Arg(1)), "new.target");
        assert_eq!(expression(&r, &Expr::Arg(2)), "this");
        assert_eq!(expression(&r, &Expr::Arg(3)), "a0");
        assert_eq!(expression(&r, &Expr::Arg(4)), "a1");

        let method = MethodInfo {
            offset: 0,
            class_off: 0,
            proto_off: 0,
            name: "f".into(),
            header_index: 0,
            kind: FunctionKind::Function,
            code: None,
            debug_info_off: 0,
            foreign: false,
        };
        let code = CodeInfo {
            offset: 0,
            num_vregs: 0,
            num_args: 5,
            code_size: 0,
            ins_off: 0,
            try_blocks: Vec::new(),
        };
        let body = LiftedBody {
            stmts: vec![],
            is_async: false,
            is_concurrent: false,
        };
        let output = render_nested(
            r.file,
            &method,
            &code,
            &body,
            0,
            crate::decompile::NestedStyle::Arrow,
            &r.nested,
        );
        assert!(output.starts_with("(a0, a1) =>"));
    }

    #[test]
    fn render_method_signature_matches_modern_arg_names_in_body() {
        let r = renderer(true);
        let method = MethodInfo {
            offset: 0,
            class_off: 0,
            proto_off: 0,
            name: "dig".into(),
            header_index: 0,
            kind: FunctionKind::Function,
            code: None,
            debug_info_off: 0,
            foreign: false,
        };
        let class = ClassInfo {
            offset: 0,
            name: "C".into(),
            access_flags: 0,
            source_file: None,
            fields: Vec::new(),
            methods: Vec::new(),
            module_record_idx: None,
        };
        let code = CodeInfo {
            offset: 0,
            num_vregs: 0,
            num_args: 4,
            code_size: 0,
            ins_off: 0,
            try_blocks: Vec::new(),
        };
        let body = LiftedBody {
            is_async: false,
            is_concurrent: false,
            stmts: vec![Stmt::Return(Some(Expr::Arg(3)))],
        };
        let output = render_method(r.file, &class, &method, &code, &body, &r.nested);
        assert_eq!(
            output,
            "// dig @ 0x0\nfunction dig(a0) {\n    return a0;\n}\n"
        );
    }

    #[test]
    fn explicit_receiver_uses_reflect_without_reading_function_properties() {
        let r = renderer(true);
        let call = Expr::Call {
            callee: Box::new(Expr::Name("fn".into())),
            args: vec![Expr::Int(7)],
            new: false,
            this_arg: Some(Box::new(Expr::Name("ctx".into()))),
        };
        assert_eq!(
            expression(&r, &call),
            "globalThis.Reflect.apply(fn, ctx, [7])"
        );
    }

    #[test]
    fn real_proto_types_reach_functions_arrows_and_bodyless_declarations() {
        let r = renderer(true);
        let bytes = crate::test_support::read_fixture("advanced_recovery/debug.abc");
        let mut file = AbcFile::parse(bytes).unwrap();
        let mut method = file
            .classes
            .iter()
            .flat_map(|c| &c.methods)
            .next()
            .unwrap()
            .clone();
        method.name = "typed".into();
        method.kind = FunctionKind::Function;
        method.debug_info_off = 0;
        method.proto_off = file.data.len() as u32;
        file.data.extend_from_slice(&0x27u16.to_le_bytes()); // (boolean) -> int
        let code = CodeInfo {
            offset: 0,
            num_vregs: 0,
            num_args: 4,
            code_size: 0,
            ins_off: 0,
            try_blocks: vec![],
        };
        let body = LiftedBody {
            stmts: vec![Stmt::Return(Some(Expr::Arg(3)))],
            is_async: false,
            is_concurrent: false,
        };
        let function = render_method(&file, &file.classes[0], &method, &code, &body, &r.nested);
        assert!(
            function.contains("function typed(a0: boolean): int {"),
            "{function}"
        );
        let arrow = render_nested(
            &file,
            &method,
            &code,
            &body,
            0,
            crate::decompile::NestedStyle::Arrow,
            &r.nested,
        );
        assert!(arrow.starts_with("(a0: boolean): int => {"), "{arrow}");
        method.code = None;
        let declaration =
            crate::decompile::decompile_method(&file, &file.classes[0], &method, None, &r.nested)
                .unwrap();
        assert!(
            declaration.contains("declare function typed(a0: boolean): int;"),
            "{declaration}"
        );

        file.header.kind = crate::file::HeaderKind::Legacy;
        let code = CodeInfo {
            num_args: 2,
            ..code
        };
        let body = LiftedBody {
            stmts: vec![Stmt::Return(Some(Expr::Arg(0)))],
            is_async: false,
            is_concurrent: false,
        };
        let function = render_method(&file, &file.classes[0], &method, &code, &body, &r.nested);
        assert!(function.contains("typed(a1: boolean): int {"), "{function}");
        assert!(function.contains("return this;"), "{function}");
    }

    #[test]
    fn unknown_proto_arity_never_misaligns_parameter_types() {
        let r = renderer(true);
        let bytes = crate::test_support::read_fixture("advanced_recovery/debug.abc");
        let mut file = AbcFile::parse(bytes).unwrap();
        let mut method = file
            .classes
            .iter()
            .flat_map(|c| &c.methods)
            .next()
            .unwrap()
            .clone();
        method.debug_info_off = 0;
        method.proto_off = file.data.len() as u32;
        file.data.extend_from_slice(&0x27u16.to_le_bytes());
        let code = CodeInfo {
            offset: 0,
            num_vregs: 0,
            num_args: 5,
            code_size: 0,
            ins_off: 0,
            try_blocks: vec![],
        };
        let body = LiftedBody {
            stmts: vec![],
            is_async: false,
            is_concurrent: false,
        };
        let arrow = render_nested(
            &file,
            &method,
            &code,
            &body,
            0,
            crate::decompile::NestedStyle::Arrow,
            &r.nested,
        );
        assert!(arrow.contains("(a0, a1): int =>"), "{arrow}");
    }

    #[test]
    fn function_body_declares_sorted_unique_registers_and_names_lexvars() {
        let r = renderer(true);
        let method = MethodInfo {
            offset: 0,
            class_off: 0,
            proto_off: 0,
            name: "f".into(),
            header_index: 0,
            kind: FunctionKind::Function,
            code: None,
            debug_info_off: 0,
            foreign: false,
        };
        let code = CodeInfo {
            offset: 0,
            num_vregs: 0,
            num_args: 3,
            code_size: 0,
            ins_off: 0,
            try_blocks: Vec::new(),
        };
        let body = LiftedBody {
            is_async: false,
            is_concurrent: false,
            stmts: vec![Stmt::Return(Some(Expr::Bin(
                "+",
                Box::new(Expr::Reg(8)),
                Box::new(Expr::Call {
                    callee: Box::new(Expr::Reg(2)),
                    args: vec![Expr::Reg(1), Expr::Reg(8)],
                    new: false,
                    this_arg: Some(Box::new(Expr::Reg(2))),
                }),
            )))],
        };
        let output = render_nested(
            r.file,
            &method,
            &code,
            &body,
            0,
            crate::decompile::NestedStyle::Arrow,
            &r.nested,
        );
        assert!(output.starts_with("() => {\n    let v1, v2, v8;\n"));
        assert_eq!(expression(&r, &Expr::LexVar(2, 5)), "lex2_5");
    }

    #[test]
    fn literal_member_helpers_keep_key_value_and_class_shapes() {
        let mut pending = None;
        assert_eq!(consume_string_literal(&mut pending, "first".into()), None);
        assert_eq!(pending.as_deref(), Some("first"));
        assert_eq!(
            consume_string_literal(&mut pending, "value".into()),
            Some(("first".into(), "value".into()))
        );
        assert_eq!(pending, None);
        assert_eq!(
            method_member("wanted", "internal(x) { body }").as_str(),
            "wanted(x) { body }"
        );
        assert_eq!(
            method_member("wanted", "async *internal(x) { body }"),
            "async *wanted(x) { body }"
        );
        assert_eq!(format_literal_container(true, &[]), "class {}");
        assert_eq!(
            format_literal_container(true, &["method() {}".into()]),
            "class { method() {} }"
        );
        assert_eq!(format_literal_container(false, &[]), "{}");
        assert_eq!(renderer(true).render_object_lit(0, false), "{}");
    }

    #[test]
    fn object_property_keys_are_quoted_when_needed_and_proto_stays_own() {
        assert_eq!(object_property_key("ordinaryName"), "ordinaryName");
        assert_eq!(
            object_property_key("ability.want.params.uiExtensionType"),
            "\"ability.want.params.uiExtensionType\""
        );
        assert_eq!(
            object_property_key("X-Request-Source"),
            "\"X-Request-Source\""
        );
        assert_eq!(object_property_key("__proto__"), "[\"__proto__\"]");

        let r = renderer(true);
        let object = Expr::Object(vec![
            (
                "ability.want.params.uiExtensionType".into(),
                Expr::Special("null"),
            ),
            ("X-Request-Source".into(), Expr::Special("null")),
            ("__proto__".into(), Expr::Special("null")),
        ]);
        assert_eq!(
            expression(&r, &object),
            "{ \"ability.want.params.uiExtensionType\": null, \"X-Request-Source\": null, [\"__proto__\"]: null }"
        );
    }

    #[test]
    fn delete_unary_operator_is_separated_from_its_operand() {
        let r = renderer(true);
        let delete = Expr::Un(
            "delete",
            Box::new(Expr::Index(
                Box::new(Expr::Name("object".into())),
                Box::new(Expr::Name("key".into())),
            )),
        );
        assert_eq!(expression(&r, &delete), "delete object[key]");
    }

    #[test]
    fn closure_and_numeric_receivers_are_parenthesized() {
        let r = renderer(true);
        let member = Expr::Member(Box::new(Expr::Closure { method_off: 0 }), "call".into());
        let index = Expr::Index(
            Box::new(Expr::Closure { method_off: 0 }),
            Box::new(Expr::Int(0)),
        );
        assert_eq!(
            expression(&r, &member),
            "(() => { throw new Error(\"Unresolved closure @ 0x0\"); }).call"
        );
        assert_eq!(
            expression(&r, &index),
            "(() => { throw new Error(\"Unresolved closure @ 0x0\"); })[0]"
        );

        let integer_member = Expr::Member(Box::new(Expr::Int(1)), "toString".into());
        let negative_float = Expr::Index(Box::new(Expr::Float(-2.5)), Box::new(Expr::Int(0)));
        assert_eq!(expression(&r, &integer_member), "(1).toString");
        assert_eq!(expression(&r, &negative_float), "(-2.5)[0]");
    }

    #[test]
    fn new_call_callee_and_else_if_render_legally() {
        let r = renderer(true);
        let factory_call = Expr::Call {
            callee: Box::new(Expr::Name("factory".into())),
            args: vec![],
            new: false,
            this_arg: None,
        };
        let construct = Expr::Call {
            callee: Box::new(factory_call),
            args: vec![],
            new: true,
            this_arg: None,
        };
        assert_eq!(expression(&r, &construct), "new (factory())()");

        let stmt = Stmt::If(
            Expr::Name("a".into()),
            vec![Stmt::Return(None)],
            vec![Stmt::If(
                Expr::Name("b".into()),
                vec![Stmt::Return(None)],
                vec![],
            )],
        );
        let mut output = String::new();
        r.render_stmt(&stmt, 0, &mut output);
        assert_eq!(
            output,
            "if (a) {\n    return;\n} else if (b) {\n    return;\n}\n"
        );
    }

    #[test]
    fn multiline_closure_indentation_tracks_statement_depth() {
        assert_eq!(
            reindent_multiline("() => {\n    return;\n}", 2),
            "() => {\n            return;\n        }"
        );
    }

    fn debug_name_fixture(
        bytecode: &[u8],
        args: u32,
        params: &[Option<&str>],
        locals: &[(u8, u32, u32, &str)],
    ) -> (AbcFile, MethodInfo) {
        fn uleb(mut value: u32, bytes: &mut Vec<u8>) {
            loop {
                let next = (value & 127) as u8;
                value >>= 7;
                bytes.push(next | if value == 0 { 0 } else { 128 });
                if value == 0 {
                    return;
                }
            }
        }
        fn string(value: &str, bytes: &mut Vec<u8>) -> u32 {
            let off = bytes.len() as u32;
            uleb(value.len() as u32 * 2 + 1, bytes);
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0);
            off
        }
        let mut data = vec![0; 68];
        let ins_off = data.len() as u32;
        data.extend_from_slice(bytecode);
        let parameter_offsets: Vec<_> = params
            .iter()
            .map(|p| p.map_or(0, |p| string(p, &mut data)))
            .collect();
        let mut events = Vec::new();
        for (reg, start, end, name) in locals {
            let off = string(name, &mut data);
            events.push((*start, 3, *reg, off));
            events.push((*end, 5, *reg, 0));
        }
        events.sort_by_key(|(pc, op, _, _)| (*pc, std::cmp::Reverse(*op)));
        let mut pc = 0;
        let mut program = Vec::new();
        let mut pool = Vec::new();
        for (next_pc, op, reg, name_off) in events {
            if next_pc > pc {
                program.push(1);
                uleb(next_pc - pc, &mut pool);
                pc = next_pc;
            }
            program.extend([op, reg]);
            if op == 3 {
                uleb(name_off, &mut pool);
                pool.push(0);
            }
        }
        program.push(0);
        let debug_info_off = data.len() as u32;
        data.push(1);
        uleb(params.len() as u32, &mut data);
        for offset in parameter_offsets {
            uleb(offset, &mut data);
        }
        uleb(pool.len() as u32, &mut data);
        data.extend(pool);
        data.push(0);
        let program_off = data.len() as u32;
        data[64..68].copy_from_slice(&program_off.to_le_bytes());
        data.extend(program);
        let mut header = renderer(true).file.header.clone();
        header.num_lnps = 1;
        header.lnp_idx_off = 64;
        let file = AbcFile {
            data,
            header,
            class_index: vec![],
            index_headers: vec![],
            classes: vec![],
        };
        let method = MethodInfo {
            offset: 0,
            class_off: 0,
            proto_off: 0,
            name: "test".into(),
            header_index: 0,
            kind: FunctionKind::Function,
            debug_info_off,
            foreign: false,
            code: Some(CodeInfo {
                offset: 0,
                num_vregs: 2,
                num_args: args,
                ins_off,
                code_size: bytecode.len() as u32,
                try_blocks: vec![],
            }),
        };
        (file, method)
    }

    #[test]
    fn debug_parameters_reserve_existing_names_and_do_not_rewrite_strings() {
        let (file, method) = debug_name_fixture(
            &[0x64],
            5,
            &[None, None, None, Some("item"), Some("item")],
            &[],
        );
        let body = LiftedBody {
            is_async: false,
            is_concurrent: false,
            stmts: vec![
                Stmt::Return(Some(Expr::Bin(
                    "+",
                    Box::new(Expr::Arg(3)),
                    Box::new(Expr::Arg(4)),
                ))),
                Stmt::Expr(Expr::Str("a0 item v0".into())),
            ],
        };
        let r = renderer(true);
        let output = render_nested(
            &file,
            &method,
            method.code.as_ref().unwrap(),
            &body,
            0,
            crate::decompile::NestedStyle::Arrow,
            &r.nested,
        );
        assert!(output.starts_with("(item, item_a1) =>"), "{output}");
        assert!(output.contains("return item + item_a1;"));
        assert!(output.contains("\"a0 item v0\""));

        let (file, method) = debug_name_fixture(&[0x64], 5, &[Some("for"), Some("v0")], &[]);
        let body = LiftedBody {
            is_async: false,
            is_concurrent: false,
            stmts: vec![Stmt::Return(Some(Expr::Reg(0)))],
        };
        let names =
            NameContext::for_method(&file, &method, method.code.as_ref().unwrap(), &body, 3);
        assert_eq!(names.arg(3, true), "a0");
        assert_eq!(names.arg(4, true), "v0_a1");

        let (file, method) = debug_name_fixture(&[0x64], 4, &[Some("a0")], &[]);
        let body = LiftedBody {
            is_async: false,
            is_concurrent: false,
            stmts: vec![Stmt::Return(Some(Expr::Name("a0".into())))],
        };
        let names =
            NameContext::for_method(&file, &method, method.code.as_ref().unwrap(), &body, 3);
        assert_eq!(names.arg(3, true), "a0_a0");
    }

    #[test]
    fn debug_parameter_prologue_is_followed_and_copy_removed() {
        // mov v0, a3; lda v0; return
        let (file, method) =
            debug_name_fixture(&[0x44, 0x50, 0x60, 0, 0x64], 4, &[], &[(0, 2, 5, "input")]);
        let body = LiftedBody {
            is_async: false,
            is_concurrent: false,
            stmts: vec![
                Stmt::Expr(Expr::Assign(Box::new(Expr::Reg(0)), Box::new(Expr::Arg(3)))),
                Stmt::Return(Some(Expr::Reg(0))),
            ],
        };
        let r = renderer(true);
        let output = render_nested(
            &file,
            &method,
            method.code.as_ref().unwrap(),
            &body,
            0,
            crate::decompile::NestedStyle::Arrow,
            &r.nested,
        );
        assert_eq!(output, "(input) => {\n    return input;\n}");

        // 原始参数槽再次读取时，不能将参数副本合并为同一个变量。
        let (file, method) =
            debug_name_fixture(&[0x44, 0x50, 0x60, 5, 0x64], 4, &[], &[(0, 2, 5, "input")]);
        let names =
            NameContext::for_method(&file, &method, method.code.as_ref().unwrap(), &body, 3);
        assert_eq!(names.arg(3, true), "input");
        assert_eq!(names.reg(0), "v0");
        assert!(names.parameter_copies.is_empty());
    }

    #[test]
    fn debug_reused_registers_and_out_of_scope_temporaries_keep_fallbacks() {
        let (file, method) = debug_name_fixture(
            &[0x60, 0, 0x60, 0, 0x64],
            3,
            &[],
            &[(0, 0, 2, "same"), (0, 2, 5, "same")],
        );
        let body = LiftedBody {
            is_async: false,
            is_concurrent: false,
            stmts: vec![Stmt::Return(Some(Expr::Reg(0)))],
        };
        let names =
            NameContext::for_method(&file, &method, method.code.as_ref().unwrap(), &body, 3);
        assert_eq!(names.reg(0), "v0");
        let (file, method) =
            debug_name_fixture(&[0x60, 0, 0x60, 0, 0x64], 3, &[], &[(0, 2, 5, "later")]);
        let names =
            NameContext::for_method(&file, &method, method.code.as_ref().unwrap(), &body, 3);
        assert_eq!(names.reg(0), "v0");
        let (file, method) = debug_name_fixture(&[0x60, 0, 0x64], 3, &[], &[(0, 0, 3, "total")]);
        let names =
            NameContext::for_method(&file, &method, method.code.as_ref().unwrap(), &body, 3);
        assert_eq!(names.reg(0), "total");
    }

    #[test]
    fn await_precedence_and_finally_body_are_preserved() {
        let r = renderer(true);
        let await_sum = Expr::Await(Box::new(Expr::Bin(
            "+",
            Box::new(Expr::Arg(3)),
            Box::new(Expr::Int(2)),
        )));
        assert_eq!(expression(&r, &await_sum), "await (a0 + 2)");
        for op in ["*", "**"] {
            let expr = Expr::Await(Box::new(Expr::Bin(
                op,
                Box::new(Expr::Arg(3)),
                Box::new(Expr::Int(2)),
            )));
            assert_eq!(expression(&r, &expr), format!("await (a0 {op} 2)"));
        }
        let member = Expr::Member(
            Box::new(Expr::Await(Box::new(Expr::Arg(3)))),
            "value".into(),
        );
        assert_eq!(expression(&r, &member), "(await a0).value");
        let mut output = String::new();
        r.render_stmt(
            &Stmt::TryFinally(
                vec![Stmt::Return(Some(await_sum))],
                vec![Stmt::Throw(Expr::Name("failure".into()))],
            ),
            0,
            &mut output,
        );
        assert_eq!(
            output,
            "try {\n    return await (a0 + 2);\n} finally {\n    throw failure;\n}\n"
        );
    }

    #[test]
    fn repeated_closures_remain_expressions_and_over_budget_is_explicit() {
        let r = renderer(true);
        r.nested
            .borrow_mut()
            .map
            .insert(3, "() => { return 7; }".into());
        let closure = Expr::Closure { method_off: 1 };
        assert_eq!(expression(&r, &closure), "() => { return 7; }");
        assert_eq!(expression(&r, &closure), "() => { return 7; }");
        r.nested.borrow_mut().closure_emitted_bytes = NestedCache::MAX_CLOSURE_BYTES;
        assert_eq!(
            expression(&r, &closure),
            "() => { throw new Error(\"Unresolved closure @ 0x1\"); }"
        );
    }

    #[test]
    fn deep_cached_object_methods_can_be_embedded_again_within_byte_budget() {
        let mut r = renderer(true);
        r.depth = 4;
        r.nested
            .borrow_mut()
            .map
            .insert(2, "method() { return 7; }".into());
        assert_eq!(
            r.nested_member(1).as_deref(),
            Some("method() { return 7; }")
        );
        assert_eq!(
            r.nested_member(1).as_deref(),
            Some("method() { return 7; }")
        );
        r.nested.borrow_mut().closure_emitted_bytes = NestedCache::MAX_CLOSURE_BYTES;
        assert!(r.nested_member(1).is_none());
        r.depth = NestedCache::MAX_DEPTH;
        assert!(r.nested_member(1).is_none());
    }
}
