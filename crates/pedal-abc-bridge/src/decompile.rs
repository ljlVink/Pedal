//! 伪 ArkTS 反编译：CFG → 表达式提升 → 结构化 → 打印
//!
//! @author ljlVink

pub mod cfg;
pub(crate) mod class_recovery;
pub mod lift;
pub mod printer;
mod signatures;
pub mod structure;

use crate::error::AbcResult;
use crate::file::{AbcFile, ClassInfo, MethodInfo, ModuleRecord};

/// 反编译产物：一个 record（源文件）的伪 ArkTS
#[derive(Debug, Clone)]
pub struct DecompiledRecord {
    /// record 名（源文件路径）
    pub name: String,
    /// 伪 ArkTS 文本
    pub source: String,
}

/// record 的模块 record（moduleRecordIdx 字段值即 literal 偏移）
fn module_record_of(file: &AbcFile, class: &ClassInfo) -> Option<ModuleRecord> {
    let off = class.module_record_idx?;
    file.read_module_record(off).ok()
}

/// 反编译单个 record
pub fn decompile_record(file: &AbcFile, class: &ClassInfo) -> AbcResult<DecompiledRecord> {
    let module = module_record_of(file, class);
    let mut units = Vec::new();
    let recovery = std::rc::Rc::new(class_recovery::RecordRecovery::build(
        file,
        class,
        module.as_ref(),
    ));
    // record 级共享：闭包/类成员展开缓存 + 额度
    let nested = std::rc::Rc::new(std::cell::RefCell::new(printer::NestedCache {
        recovery: Some(recovery.clone()),
        map: std::collections::HashMap::new(),
        closure_budget: 256,
        closure_bytes: 0,
        member_budget: 256,
        member_bytes: 0,
        closure_emitted_bytes: 0,
    }));
    // 模块 import/export
    if let Some(m) = &module {
        let header = printer::render_module_header(m);
        if !header.is_empty() {
            units.push(header);
        }
    }
    // 旧格式保留名称启发式；现代格式必须由 defineclass 证明归属。
    // `#&#Name` 同样用于 namespace 的 IIFE，不能仅凭名称把闭包移到类作用域。
    let class_init = class
        .methods
        .iter()
        .find(|m| matches!(classify_method(&m.name), MethodFamily::ClassInit(_)));
    let class_members: Vec<&MethodInfo> = class
        .methods
        .iter()
        .filter(|m| matches!(classify_method(&m.name), MethodFamily::ClassMethod { .. }))
        .collect();
    let mut synthesized_class: Option<String> = None;
    let mut class_name_out: Option<String> = None;
    if file.header.kind == crate::file::HeaderKind::Legacy
        && !recovery.has_classes()
        && let Some(init) = class_init
        && !class_members.is_empty()
        && let Some((text, cname)) = synthesize_class(file, class, init, &class_members, &nested)
    {
        class_name_out = Some(cname);
        synthesized_class = Some(text);
    }

    // 各函数（func_main_0 顶层化，渲染在最后）
    let mut top_level: Option<String> = None;
    let mut lex_consts: std::collections::HashMap<(u32, u32), String> =
        std::collections::HashMap::new();
    for m in &class.methods {
        if m.name == "func_main_0" {
            if let Some(code) = &m.code {
                let bytes = code.instructions(&file.data);
                let legacy = file.header.kind == crate::file::HeaderKind::Legacy;
                match crate::disasm::decode_with(bytes, legacy) {
                    Ok(ins) if ins.len() <= 2_000 => {
                        let graph = cfg::build(&ins, bytes.len(), &code.try_blocks);
                        let mut body = lift::lift_with_lexical_names(
                            file,
                            m,
                            code,
                            &graph,
                            module.as_ref(),
                            recovery.lexical_names(m.offset),
                        );
                        recovery.rewrite_body(m.offset, &mut body);
                        structure::polish(&mut body.stmts);
                        recovery.finish_main(file, &mut body);
                        // 顶层筛选：类导出/机器语句剔除，词法环境字面量记名
                        let mut filtered = Vec::new();
                        for st in std::mem::take(&mut body.stmts) {
                            match &st {
                                lift::Stmt::Expr(lift::Expr::Assign(target, val))
                                    if matches!(target.as_ref(), lift::Expr::LexVar(_, _))
                                        && matches!(
                                            val.as_ref(),
                                            lift::Expr::Str(_)
                                                | lift::Expr::Int(_)
                                                | lift::Expr::Bool(_)
                                                | lift::Expr::Special(_)
                                        ) =>
                                {
                                    if let lift::Expr::LexVar(d, s) = target.as_ref() {
                                        let name = format!("lex{d}_{s}");
                                        lex_consts.insert((*d, *s), name.clone());
                                        filtered.push(lift::Stmt::Declare(
                                            name,
                                            Some(val.as_ref().clone()),
                                        ));
                                    }
                                }
                                // 类导出（含 default）：类由合成渲染表达
                                lift::Stmt::Expr(lift::Expr::Assign(target, _))
                                    if matches!(target.as_ref(), lift::Expr::Name(n)
                                        if class_name_out.as_deref().is_some_and(|c|
                                            n.trim_start_matches("export:") == c)
                                            || matches!(target.as_ref(), lift::Expr::Name(n)
                                                if n == "export:default")) => {}
                                // defineclass 机器调用：lexenv 空对象/Boolean 包装
                                lift::Stmt::Expr(lift::Expr::Call { callee, .. })
                                    if match callee.as_ref() {
                                        lift::Expr::TemplateLit(_) => true,
                                        lift::Expr::Name(n) => n == "Boolean",
                                        _ => false,
                                    } => {}
                                // 模块顶层隐式 return
                                lift::Stmt::Return(None) => {}
                                _ => filtered.push(st),
                            }
                        }
                        body.stmts = filtered;
                        top_level = Some(printer::render_top_level(file, &body, &nested));
                    }
                    Ok(_) => {
                        // 顶层初始化同样需要保留超限回退，不能静默丢失整份源码。
                        top_level = Some(printer::render_large_method(file, m, code, bytes));
                    }
                    Err(error) => {
                        top_level = Some(format!(
                            "// method {:?} @ {:#x}: bytecode decode failed: {error}\n",
                            m.name, m.offset
                        ));
                    }
                }
            }
            continue;
        }
        if recovery.is_member(m.offset) {
            continue;
        }
        if synthesized_class.is_some()
            && matches!(
                classify_method(&m.name),
                MethodFamily::ClassInit(_) | MethodFamily::ClassMethod { .. }
            )
        {
            continue;
        }
        let src = decompile_method(file, class, m, module.as_ref(), &nested)?;
        if !src.is_empty() {
            units.push(src);
        }
    }
    if let Some(cls) = synthesized_class {
        units.push(cls);
    }
    if let Some(top) = top_level {
        let mut top = top;
        for name in lex_consts.values() {
            top = top.replace(&format!("let {name} = "), &format!("const {name} = "));
        }
        units.push(top);
    }
    // 词法环境常量引用整体替换
    if !lex_consts.is_empty() {
        let mut replaced = Vec::with_capacity(units.len());
        for u in units {
            let mut u = u;
            for ((d, s), name) in &lex_consts {
                u = u.replace(&format!("lex[{d}][{s}]"), name);
            }
            replaced.push(u);
        }
        units = replaced;
    }
    Ok(DecompiledRecord {
        name: class.name.clone(),
        source: units.join("\n"),
    })
}

/// es2abc 符号修饰分类
#[derive(Debug, PartialEq)]
pub enum MethodFamily {
    /// `#&#Name`：类初始化函数（往构造器上挂静态成员）
    ClassInit(String),
    /// `#&@N<sep>#name`：类方法（sep='*' 静态）
    ClassMethod { static_: bool, name: String },
    /// 普通模块/record 函数
    Other,
}

/// 解析 es2abc 修饰名
pub fn classify_method(name: &str) -> MethodFamily {
    if let Some(rest) = name.strip_prefix("#&#")
        && !rest.is_empty()
    {
        return MethodFamily::ClassInit(rest.to_string());
    }
    if let Some(rest) = name.strip_prefix("#&@") {
        // `0*#info` → idx、分隔符、方法名
        if let Some(sep_pos) = rest.find('#') {
            let mid = &rest[..sep_pos];
            let mname = &rest[sep_pos + 1..];
            if !mname.is_empty() {
                let static_ = mid.contains('*');
                return MethodFamily::ClassMethod {
                    static_,
                    name: mname.to_string(),
                };
            }
        }
    }
    MethodFamily::Other
}

/// 嵌套函数渲染样式
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NestedStyle {
    /// 闭包：`(a0, a1) => { ... }`
    Arrow,
    /// 类成员：`name(a1) { ... }`
    Method,
}

/// 反编译方法池中偏移指向的函数（闭包/类成员递归展开）。
/// 深度/体积超限时返回 None，由调用方退化为注释。
pub fn render_nested_function(
    file: &crate::file::AbcFile,
    method_off: u32,
    depth: u8,
    style: NestedStyle,
    cache: &std::rc::Rc<std::cell::RefCell<printer::NestedCache>>,
) -> Option<String> {
    if depth > printer::NestedCache::MAX_DEPTH {
        return None;
    }
    let key = (method_off as u64) * 2 + u64::from(style == NestedStyle::Arrow);
    if let Some(hit) = cache.borrow().map.get(&key) {
        return Some(hit.clone());
    }
    // 额度用尽：防止嵌套展开乘性放大
    let arrow = style == NestedStyle::Arrow;
    if cache.borrow().exhausted(arrow) {
        return None;
    }
    cache.borrow_mut().reserve(arrow);
    let m = file.method_at(method_off).ok()?;
    let code = m.code.as_ref()?;
    if code.code_size > 8_000 {
        return None;
    }
    let bytes = code.instructions(&file.data);
    let legacy = file.header.kind == crate::file::HeaderKind::Legacy;
    let ins = crate::disasm::decode_with(bytes, legacy).ok()?;
    if ins.len() > 4_096 {
        return None;
    }
    let graph = cfg::build(&ins, bytes.len(), &code.try_blocks);
    let recovery = cache.borrow().recovery.clone();
    let module = recovery.as_ref().and_then(|r| r.module(file));
    let names = recovery
        .as_ref()
        .map(|r| r.lexical_names(m.offset))
        .unwrap_or_default();
    let mut body = lift::lift_with_lexical_names(file, &m, code, &graph, module.as_ref(), names);
    if let Some(recovery) = recovery {
        recovery.rewrite_body(m.offset, &mut body);
    }
    structure::polish(&mut body.stmts);
    let text = printer::render_nested(file, &m, code, &body, depth, style, cache);
    {
        let mut c = cache.borrow_mut();
        c.record_bytes(arrow, text.len());
        c.map.insert(key, text.clone());
    }
    Some(text)
}

/// 合成 `export class` 文本：静态方法来自 `#&@N*#` 族，
/// 静态字段声明来自初始化器体内的 `a3.X = 字面量`
fn synthesize_class(
    file: &AbcFile,
    class: &ClassInfo,
    init: &MethodInfo,
    members: &[&MethodInfo],
    nested: &std::rc::Rc<std::cell::RefCell<printer::NestedCache>>,
) -> Option<(String, String)> {
    let cname = match classify_method(&init.name) {
        MethodFamily::ClassInit(n) => sanitize_name(&n),
        _ => return None,
    };
    let mut out = format!("export class {cname} {{\n");
    let module = module_record_of(file, class);
    // 方法��集合：与成员同名的占位字段声明（a3.X = undefined）不再输出
    let method_names: std::collections::HashSet<String> = members
        .iter()
        .filter_map(|m| match classify_method(&m.name) {
            MethodFamily::ClassMethod { name, .. } => Some(name),
            _ => None,
        })
        .collect();
    // 静态字段：初始化器体 `a3.X = 字面量`
    if let Some(code) = &init.code {
        let bytes = code.instructions(&file.data);
        let legacy = file.header.kind == crate::file::HeaderKind::Legacy;
        if let Ok(ins) = crate::disasm::decode_with(bytes, legacy) {
            let graph = cfg::build(&ins, bytes.len(), &code.try_blocks);
            let body = lift::lift(file, class, init, code, &graph, None);
            let mut seen = std::collections::HashSet::new();
            for st in &body.stmts {
                if let lift::Stmt::Expr(lift::Expr::Assign(target, val)) = st
                    && let lift::Expr::Member(_, key) = target.as_ref()
                    && matches!(
                        val.as_ref(),
                        lift::Expr::Str(_)
                            | lift::Expr::Int(_)
                            | lift::Expr::Bool(_)
                            | lift::Expr::Special(_)
                    )
                    && seen.insert(key.clone())
                    && !method_names.contains(key)
                {
                    out.push_str(&format!("    static {key};\n"));
                }
            }
        }
    }
    // 静态方法体（沿用 decompile_method 的渲染，重排为类成员缩进）
    for m in members {
        let src = decompile_method(file, class, m, module.as_ref(), nested).ok()?;
        let modifier = if matches!(
            classify_method(&m.name),
            MethodFamily::ClassMethod { static_: true, .. }
        ) {
            "static "
        } else {
            ""
        };
        let mut in_body = false;
        for line in src.lines() {
            if !in_body && (line.starts_with("//") || line.is_empty()) {
                continue;
            }
            if !in_body {
                let line = line.replacen("function ", "", 1);
                out.push_str(&format!("    {modifier}{line}\n"));
                if line.contains('{') {
                    in_body = true;
                }
            } else if !line.is_empty() {
                out.push_str(&format!("    {line}\n"));
            }
        }
    }
    out.push_str("}\n");
    Some((out, cname))
}

/// 类名清理（保留字母数字下划线）
fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "Anonymous".into()
    } else {
        cleaned
    }
}

/// 反编译单个方法
pub fn decompile_method(
    file: &AbcFile,
    class: &ClassInfo,
    m: &MethodInfo,
    module: Option<&ModuleRecord>,
    nested: &std::rc::Rc<std::cell::RefCell<printer::NestedCache>>,
) -> AbcResult<String> {
    let Some(code) = &m.code else {
        return Ok(printer::render_declaration(file, m).unwrap_or_default());
    };
    let bytes = code.instructions(&file.data);
    let legacy = file.header.kind == crate::file::HeaderKind::Legacy;
    let ins = crate::disasm::decode_with(bytes, legacy)?;
    // 超大方法（巨型 switch/分发器）走快速路径：完整反汇编 + 截断伪代码，
    // 避免 structuring 的二次方开销
    if bytes.len() > 20_000 || ins.len() > 4_000 {
        return Ok(printer::render_large_method(file, m, code, bytes));
    }
    let graph = cfg::build(&ins, bytes.len(), &code.try_blocks);
    let names = nested
        .borrow()
        .recovery
        .as_ref()
        .map(|r| r.lexical_names(m.offset))
        .unwrap_or_default();
    let mut body = lift::lift_with_lexical_names(file, m, code, &graph, module, names);
    if let Some(recovery) = nested.borrow().recovery.clone() {
        recovery.rewrite_body(m.offset, &mut body);
    }
    structure::polish(&mut body.stmts);
    structure::recover_switches(&mut body.stmts);
    Ok(printer::render_method(file, class, m, code, &body, nested))
}
