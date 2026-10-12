//! 伪 ArkTS 打印：表达式/语句 → 文本；模块 import/export 渲染
//!
//! @author ljlVink

use crate::decompile::lift::{Expr, Stmt};
use crate::file::{AbcFile, ClassInfo, CodeInfo, LiteralValue, MethodInfo};
use std::cell::RefCell;
use std::collections::HashMap;

/// 渲染上下文：持有文件以便展开 literal。
/// literal 缓存兼作循环守卫：literal 数组可自引用/共享，
/// 不加记忆化会在 DAG 展开时指数爆炸
/// 嵌套函数（闭包/类成员）展开缓存，单次方法渲染内共享
pub struct NestedCache {
    pub(crate) map: HashMap<u64, String>,
    /// 闭包展开剩余额度 / 已产字节
    pub(crate) closure_budget: usize,
    pub(crate) closure_bytes: usize,
    /// 类成员展开剩余额度 / 已产字节
    pub(crate) member_budget: usize,
    pub(crate) member_bytes: usize,
    /// 已展开过的方法偏移（含闭包：同 offset 只展开一次）
    pub(crate) expanded: std::collections::HashSet<u32>,
    /// 已嵌入正文的闭包偏移（正文只嵌一次，其余引用降级为注释，防输出乘性膨胀）
    pub(crate) closure_embedded: std::collections::HashSet<u32>,
}

impl NestedCache {
    /// 单 record 闭包/类成员展开文本上限
    pub const MAX_CLOSURE_BYTES: usize = 256 * 1024;
    pub const MAX_MEMBER_BYTES: usize = 64 * 1024;

    /// 展开是否超限
    /// 闭包正文嵌入（仅首次返回 true）
    pub(crate) fn embed_closure(&mut self, off: u32) -> bool {
        self.closure_embedded.insert(off)
    }

    pub(crate) fn exhausted(&self, arrow: bool) -> bool {
        if arrow {
            self.closure_budget == 0 || self.closure_bytes >= Self::MAX_CLOSURE_BYTES
        } else {
            self.member_budget == 0 || self.member_bytes >= Self::MAX_MEMBER_BYTES
        }
    }

    pub(crate) fn charge(&mut self, arrow: bool, len: usize) {
        if arrow {
            self.closure_budget = self.closure_budget.saturating_sub(1);
            self.closure_bytes = self.closure_bytes.saturating_add(len);
        } else {
            self.member_budget = self.member_budget.saturating_sub(1);
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
        crate::file::FunctionKind::AsyncFunction => "async ",
        crate::file::FunctionKind::AsyncGeneratorFunction => "async *",
        crate::file::FunctionKind::ConcurrentFunction => "/* @Concurrent */ ",
        _ => "",
    };
    // 实例方法 a0 是 this（参数从 a1 起）；类静态方法 a0..a2 是
    // (env, 类引用, ...) 机器参数（参数从 a3 起）
    let class_static = m.name.starts_with("#&@") && m.name.contains('*');
    let this_arg0 = !class_static && m.name.contains('<');
    let first_param = if class_static {
        3.min(code.num_args as usize)
    } else {
        usize::from(this_arg0)
    };
    let params: Vec<String> = (first_param..code.num_args as usize)
        .map(|i| format!("a{i}"))
        .collect();
    out.push_str(&format!("// {} @ {:#x}\n", m.name, m.offset));
    out.push_str(&format!(
        "{modifier}function {name}({}) {{\n",
        params.join(", ")
    ));
    let renderer = Renderer {
        file,
        lit_cache: RefCell::new(HashMap::new()),
        nested: nested.clone(),
        depth: 0,
        this_arg0,
    };
    for s in &body.stmts {
        renderer.render_stmt(s, 1, &mut out);
    }
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
    let renderer = Renderer {
        file,
        lit_cache: RefCell::new(HashMap::new()),
        nested: nested.clone(),
        depth,
        this_arg0: m.name.contains('<'),
    };
    match style {
        crate::decompile::NestedStyle::Arrow => {
            let params: Vec<String> = (0..code.num_args as usize)
                .map(|i| format!("a{i}"))
                .collect();
            let mut out = format!("({}) => {{\n", params.join(", "));
            for s in &body.stmts {
                renderer.render_stmt(s, 1, &mut out);
            }
            out.push('}');
            out
        }
        crate::decompile::NestedStyle::Method => {
            let name = if m.name == "<ctor>" {
                "constructor".to_string()
            } else {
                demangle_or_offset(&m.name, m.offset)
            };
            let first_param = usize::from(renderer.this_arg0);
            let params: Vec<String> = (first_param..code.num_args as usize)
                .map(|i| format!("a{i}"))
                .collect();
            let mut out = format!("{name}({}) {{\n", params.join(", "));
            for s in &body.stmts {
                renderer.render_stmt(s, 1, &mut out);
            }
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
    };
    let mut out = String::new();
    for s in &body.stmts {
        renderer.render_stmt(s, 0, &mut out);
    }
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
        (Expr::ClassLit(x), Expr::ClassLit(y)) => x == y,
        (Expr::TemplateLit(x), Expr::TemplateLit(y)) => x == y,
        (Expr::Spread(a), Expr::Spread(b)) => expr_eq_bounded(a, b, budget),
        _ => false,
    }
}

impl Renderer<'_> {
    fn render_stmt(&self, s: &Stmt, depth: usize, out: &mut String) {
        match s {
            Stmt::Expr(e) => {
                out.push_str(&indent(depth));
                self.write_expr(e, out);
                out.push_str(";\n");
            }
            Stmt::Declare(name, init) => {
                out.push_str(&indent(depth));
                match init {
                    Some(e) => {
                        out.push_str(&format!("let {name} = "));
                        self.write_expr(e, out);
                        out.push_str(";\n");
                    }
                    None => out.push_str(&format!("let {name};\n")),
                }
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
                    out.push_str(&format!("{}}} else {{\n", indent(depth)));
                    for s in els {
                        self.render_stmt(s, depth + 1, out);
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
            Stmt::Break => out.push_str(&format!("{}break;\n", indent(depth))),
            Stmt::Continue => out.push_str(&format!("{}continue;\n", indent(depth))),
            Stmt::Comment(text) => {
                out.push_str(&indent(depth));
                out.push_str(&format!("/* {} */\n", text.replace('\n', " ")));
            }
        }
    }

    /// 表达式写入共享缓冲区：深链场景下 format! 每层都会拷贝子串，
    /// 总成本 O(n²)；追加写入为 O(n)
    fn write_expr(&self, e: &Expr, out: &mut String) {
        match e {
            Expr::Reg(n) => out.push_str(&format!("v{n}")),
            Expr::Arg(n) => {
                if *n == 0 && self.this_arg0 {
                    out.push_str("this");
                } else {
                    out.push_str(&format!("a{n}"));
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
                out.push('(');
                self.write_expr(a, out);
                out.push(' ');
                out.push_str(op);
                out.push(' ');
                self.write_expr(b, out);
                out.push(')');
            }
            Expr::Un(op, a) => {
                if op == &"typeof" {
                    out.push_str("typeof ");
                    self.write_expr(a, out);
                } else {
                    out.push('(');
                    out.push_str(op);
                    self.write_expr(a, out);
                    out.push(')');
                }
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
                out.push('(');
                self.write_expr(t, out);
                out.push_str(" = ");
                self.write_expr(v, out);
                out.push(')');
            }
            Expr::Cond(c, a, b) => {
                out.push('(');
                self.write_expr(c, out);
                out.push_str(" ? ");
                self.write_expr(a, out);
                out.push_str(" : ");
                self.write_expr(b, out);
                out.push(')');
            }
            Expr::Member(o, name) => {
                let needs_brackets = name.is_empty()
                    || !name
                        .chars()
                        .next()
                        .map(|c| c.is_alphabetic() || c == '_' || c == '$')
                        .unwrap_or(false);
                if needs_brackets {
                    self.write_expr(o, out);
                    out.push_str(&format!("[{name:?}]"));
                } else {
                    self.write_expr(o, out);
                    out.push('.');
                    out.push_str(name);
                }
            }
            Expr::Index(o, k) => {
                self.write_expr(o, out);
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
                if !drop_this
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
                if *new {
                    out.push_str("new ");
                }
                self.write_expr(callee, out);
                out.push('(');
                if let Some(t) = this_arg
                    && !drop_this
                {
                    self.write_expr(t, out);
                    if !args.is_empty() {
                        out.push_str(", ");
                    }
                }
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.write_expr(a, out);
                }
                out.push(')');
            }
            Expr::Closure { method_off } => {
                let text = if self.depth < 3 {
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
                    Some(t) => {
                        if self.nested.borrow_mut().embed_closure(*method_off) {
                            out.push_str(&t);
                        } else {
                            out.push_str(&format!("/*closure@{method_off:#x} - 见上方展开*/"));
                        }
                    }
                    None => out.push_str(&format!("/*closure@{method_off:#x}*/")),
                }
            }
            Expr::LexVar(depth, slot) => out.push_str(&format!("lex[{depth}][{slot}]")),
            Expr::ArrayLit(off) => out.push_str(&self.render_array_lit(*off)),
            Expr::ObjectLit(off) => out.push_str(&self.render_object_lit(*off, false)),
            Expr::ClassLit(off) => out.push_str(&self.render_object_lit(*off, true)),
            Expr::TemplateLit(t) => out.push_str(t),
            Expr::Spread(e) => {
                out.push_str("...");
                self.write_expr(e, out);
            }
        }
    }

    /// 类成员方法体展开（深度/体积受限；展开过的方法只展开一次）
    fn nested_member(&self, method_off: u32) -> Option<String> {
        if self.depth >= 2 {
            return None;
        }
        if !self.nested.borrow_mut().expanded.insert(method_off) {
            return None;
        }
        crate::decompile::render_nested_function(
            self.file,
            method_off,
            self.depth + 1,
            crate::decompile::NestedStyle::Method,
            &self.nested.clone(),
        )
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
        let head = if is_class { "class " } else { "" };
        let mut members = Vec::new();
        let mut pending_key: Option<String> = None;
        let mut class_name = String::new();
        for v in lits.iter() {
            match v {
                LiteralValue::String(_) => {
                    if let Some(key) = pending_key.take() {
                        members.push(format!("{key}: {}", self.render_literal_value(v)));
                    }
                    pending_key = match v {
                        LiteralValue::String(o) => self.file.string_at(*o).ok(),
                        _ => None,
                    };
                    // 类字面量首个字符串通常是类名
                    if is_class
                        && class_name.is_empty()
                        && members.is_empty()
                        && let Some(n) = &pending_key
                    {
                        class_name = n.clone();
                        pending_key = None;
                    }
                }
                LiteralValue::Method(m)
                | LiteralValue::GeneratorMethod(m)
                | LiteralValue::AsyncGeneratorMethod(m) => {
                    if let Some(key) = pending_key.take() {
                        let body = self.nested_member(*m);
                        members.push(match body {
                            Some(text) => format!("{key} {text}"),
                            None => format!("{key}() {{ /*method@{m:#x}*/ }}"),
                        });
                    } else {
                        members.push(format!("/*method@{m:#x}*/"));
                    }
                }
                LiteralValue::Getter(g) => {
                    if let Some(key) = pending_key.take() {
                        let body = self.nested_member(*g);
                        members.push(match body {
                            Some(text) => format!("get {key} {text}"),
                            None => format!("get {key}() {{ /*@{g:#x}*/ }}"),
                        });
                    }
                }
                LiteralValue::Setter(sv) => {
                    if let Some(key) = pending_key.take() {
                        let body = self.nested_member(*sv);
                        members.push(match body {
                            Some(text) => format!("set {key} {text}"),
                            None => format!("set {key}() {{ /*@{sv:#x}*/ }}"),
                        });
                    }
                }
                other => {
                    if let Some(key) = pending_key.take() {
                        members.push(format!("{key}: {}", self.render_literal_value(other)));
                    }
                }
            }
        }
        let out = if is_class {
            format!("{head}{class_name} {{ {} }}", members.join("; "))
        } else {
            format!("{{ {} }}", members.join(", "))
        };
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
