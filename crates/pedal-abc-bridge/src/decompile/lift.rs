//! 表达式提升：累加器/寄存器 → 表达式树，指令 → 语句
//!
//! 采用单遍线性扫描（jadx 风格）：寄存器与累加器映射到最新表达式；
//! 控制流指令结合 CFG 做贪心结构化（if/else、while、do-while、try/catch）。
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
    /// 数组构建器（reg → 元素表，stownbyindex/starrayspread 填充）
    array_builders: std::collections::HashMap<u64, Vec<Expr>>,
    regs: Vec<Option<Expr>>,
    /// 每个寄存器的赋值次数（sta/mov 目标）：多次赋值 = 循环变量，禁止常量折叠
    assign_counts: Vec<u32>,
    stmts: Vec<Stmt>,
    num_vregs: u32,
    num_args: u32,
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
    let bytes = code.instructions(&file.data);
    let legacy = file.header.kind == crate::file::HeaderKind::Legacy;
    let ins = crate::disasm::decode_with(bytes, legacy).unwrap_or_default();
    let mut regs: Vec<Option<Expr>> = vec![None; (code.num_vregs + code.num_args + 1) as usize];
    let mut assign_counts = vec![0u32; regs.len()];
    for x in &ins {
        let mn = x.mnemonic();
        if mn == "sta" || mn.starts_with("mov") {
            // sig 名与编码字段名不同（如 mov 的 dst 在 sig 里叫 v1），
            // 统一走 sig_values：sta 取 v，mov 取 v1（首个操作数恒为目标）
            let want = if mn == "sta" { "v" } else { "v1" };
            let dst = x
                .sig_values()
                .into_iter()
                .find(|(n, _, _)| *n == want)
                .map(|(_, _, val)| val);
            if let Some(d) = dst
                && (d as usize) < assign_counts.len()
            {
                assign_counts[d as usize] += 1;
            }
        }
    }
    // 参数寄存器绑定
    for i in 0..code.num_args {
        regs[(code.num_vregs + i) as usize] = Some(Expr::Arg(i));
    }
    let mut lifter = Lifter {
        file,
        method,
        module,
        ins: &ins,
        cfg,
        acc: Expr::Special("undefined"),
        acc_persisted: false,
        lex_names: Vec::new(),
        array_builders: std::collections::HashMap::new(),
        regs,
        assign_counts,
        stmts: Vec::new(),
        num_vregs: code.num_vregs,
        num_args: code.num_args,
    };
    lifter.run();
    LiftedBody {
        stmts: lifter.stmts,
    }
}

/// 标量字面量（可安全折叠）；引用值与未知表达式保持变量名
fn is_immutable_value(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Int(_)
            | Expr::Float(_)
            | Expr::Str(_)
            | Expr::Bool(_)
            | Expr::Special("null")
            | Expr::Special("undefined")
            | Expr::Arg(_)
            | Expr::Reg(_)
            | Expr::LexVar(_, _)
    )
}

impl<'a> Lifter<'a> {
    fn v(&self, n: u64) -> Expr {
        // 内联预算：寄存器值被多次读取时克隆体按扇出乘积放大，
        // 超过节点上限退化为不透明 vN 引用，防止表达式树内存爆炸
        const MAX_INLINE_NODES: usize = 1_000;
        // 多次赋值的寄存器（循环变量）与引用值（数组/对象）不折叠为字面量：
        // 折叠会让 out.push(a) 变成 [].push(1) 且丢失循环内的状态更新
        let multi = self
            .assign_counts
            .get(n as usize)
            .map(|c| *c > 1)
            .unwrap_or(false);
        match self.regs.get(n as usize).and_then(|e| e.as_ref()) {
            Some(e) => {
                if multi
                    || !is_immutable_value(e)
                    || crate::decompile::structure::count_nodes(e) > MAX_INLINE_NODES
                {
                    Expr::Reg(n as u32)
                } else {
                    e.clone()
                }
            }
            None => {
                if (n as u32) >= self.num_vregs && (n as u32) < self.num_vregs + self.num_args {
                    Expr::Arg(n as u32 - self.num_vregs)
                } else {
                    Expr::Reg(n as u32)
                }
            }
        }
    }

    fn set_v(&mut self, n: u64, e: Expr) {
        if n as usize >= self.regs.len() {
            self.regs.resize(n as usize + 1, None);
        }
        self.regs[n as usize] = Some(e);
    }

    fn take_acc(&mut self) -> Expr {
        self.acc_persisted = false;
        std::mem::replace(&mut self.acc, Expr::Special("undefined"))
    }

    /// 词法环境槽位命名（栈顶为深度 0）
    fn lex_slot_name(&self, depth: usize, slot: usize) -> Option<String> {
        if self.lex_names.is_empty() {
            return None;
        }
        let idx = self.lex_names.len().checked_sub(depth + 1)?;
        let names = self.lex_names.get(idx)?.as_ref()?;
        let name = names.get(slot)?;
        if name.is_empty() || name.chars().all(|c| !c.is_alphanumeric()) {
            return None;
        }
        Some(format!("cap_{name}"))
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
        if let Some(m) = self.module {
            if let Some(r) = m.regular_imports.get(idx) {
                return format!("import:{}", r.local_name);
            }
            if let Some(n) = m.namespace_imports.get(idx) {
                return format!("import:* {}", n.local_name);
            }
        }
        format!("imported_{idx}")
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
        // JD arkdecompiler 式区域发射：块级语句 + 后继胶水。
        // 汇合块 = 多前驱块；循环 = 自然循环（支配者保证）
        let mut visited = vec![false; self.cfg.blocks.len()];
        let no_stop: Vec<usize> = Vec::new();
        let mut stmts = self.emit_seq(0, &no_stop, &mut visited);
        std::mem::swap(&mut stmts, &mut self.stmts);
        // run 结束：self.stmts 已包含全部语句
    }

    /// 从 entry 顺序发射直到 stop 集合或已访问块
    fn emit_seq(&mut self, entry: usize, stop: &[usize], visited: &mut Vec<bool>) -> Vec<Stmt> {
        let mut out: Vec<Stmt> = Vec::new();
        let mut bi = entry;
        loop {
            if bi >= self.cfg.blocks.len() || visited[bi] || stop.contains(&bi) {
                break;
            }
            // 循环头
            if let Some(lp) = self.cfg.loops.get(&bi).cloned() {
                visited[bi] = true;
                self.emit_block_insns(bi, &mut out);
                if lp.is_do_while {
                    // 体 = 除 header 外的循环块，按序发射
                    let mut body = Vec::new();
                    for &bb in &lp.body {
                        if bb == bi || visited[bb] {
                            continue;
                        }
                        self.emit_block_insns(bb, &mut body);
                        visited[bb] = true;
                    }
                    let cond = self.take_cond_of();
                    out.push(Stmt::DoWhile(body, cond));
                } else {
                    // while：header 的条件分支，体 = 进入循环的那条边
                    let cond = self.take_cond_of();
                    // 体入口 = header 的在循环内的后继
                    let body_entry = self.cfg.blocks[bi]
                        .succs
                        .iter()
                        .copied()
                        .find(|&x| lp.body.contains(&x) && x != bi)
                        .unwrap_or(bi);
                    let body_stop: Vec<usize> = lp.exits.clone();
                    let body = self.emit_seq(body_entry, &body_stop, visited);
                    out.push(Stmt::While(cond, body));
                }
                // 继续到出口
                match lp.exits.first() {
                    Some(&e) => {
                        bi = e;
                        continue;
                    }
                    None => break,
                }
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
                    let positive = mn.starts_with("jn");
                    let cond = self.peek_acc();
                    self.acc_persisted = true;
                    // 汇合块：两臂首个公共多前驱块（兜底：较大臂的下一块）
                    let merge = self.find_merge(t_bi, f);
                    let (then_entry, else_entry, cond) = if positive {
                        (t_bi, f, cond)
                    } else {
                        (f, t_bi, invert_cond(cond))
                    };
                    let stop = [merge];
                    let then_stmts = self.emit_seq(then_entry, &stop, visited);
                    let else_stmts = if else_entry != merge {
                        self.emit_seq(else_entry, &stop, visited)
                    } else {
                        Vec::new()
                    };
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
                m.starts_with("return") || m.starts_with("throw")
            })
            .unwrap_or(false)
    }

    /// 取条件分支块的分支条件（不清 acc）
    fn take_cond_of(&mut self) -> Expr {
        let cond = self.peek_acc();
        self.acc_persisted = true;
        cond
    }

    /// 两臂汇合块：从较大下标臂向后找第一个多前驱块
    fn find_merge(&self, a: usize, b: usize) -> usize {
        let hi = a.max(b);
        // 从 hi 起找多前驱且非自身循环头的块
        for x in hi..self.cfg.blocks.len() {
            if self.cfg.preds[x].len() >= 2 && x != a && x != b {
                return x;
            }
        }
        // 兜底：hi 的下一块
        (hi + 1).min(self.cfg.blocks.len().saturating_sub(1))
    }

    fn visit(&mut self, i: &Ins) {
        let mn = i.mnemonic();
        let mn = mn.as_str();
        match mn {
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
            "ldhole" => self.set_acc(Expr::Special("undefined")),
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
            }
            "ldai" => {
                let v = signed_imm_of(i);
                self.set_acc(Expr::Int(v));
            }
            "f64ai" | "wide.ldai.64" => {
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
            "shr2" => self.bin(">>", i),
            "ashr2" => self.bin(">>>", i),
            "and2" => self.bin("&", i),
            "or2" => self.bin("|", i),
            "xor2" => self.bin("^", i),
            "eq" => self.bin("==", i),
            "neq" => self.bin("!=", i),
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
            "stownbyname" | "definefieldbyname" => {
                // 对象.K = acc（definefield 变体语义相同）
                let v = self.reg_of(i);
                let name = self.resolve_str(i);
                let val = self.peek_acc();
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
            "in" => self.bin("in", i),
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
                let val = self.peek_acc();
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
                let val = self.peek_acc();
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
            "stobjbyvalue" => {
                // v1[v2] = acc
                let vs = self.regs_of(i);
                let obj = vs.first().copied().unwrap_or(0);
                let key = vs.get(1).copied().unwrap_or(0);
                let val = self.peek_acc();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Index(Box::new(self.v(obj)), Box::new(self.v(key)))),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "ldobjbyindex" => {
                let v = self.reg_of(i);
                let idx = self.imm_last(i);
                self.set_acc(Expr::Index(
                    Box::new(self.v(v)),
                    Box::new(Expr::Int(idx as i64)),
                ));
            }
            "stobjbyindex" => {
                let v = self.reg_of(i);
                let idx = self.imm_last(i);
                let val = self.peek_acc();
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
            "callarg0" | "callarg1" | "callarg2" | "callarg3" | "callarg4" => {
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
                let count = self.imm(i) as usize;
                let start = self.reg_of(i) as usize;
                let is_new = mn == "newobjrange";
                let callee = self.v(start as u64);
                let (this_arg, arg_start, arg_count) = if mn == "callthisrange" {
                    (
                        Some(Box::new(self.v((start + 1) as u64))),
                        start + 2,
                        count.saturating_sub(2),
                    )
                } else {
                    (None, start + 1, count.saturating_sub(1))
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
                // v = class <name> { members }（literal buffer）
                let vs = self.regs_of(i);
                let off = self.litarr_of(i).unwrap_or_else(|| {
                    // literalarray 是第二个 id 字段
                    i.sig_values()
                        .iter()
                        .find(|(n, _, _)| n.ends_with("literalarray_id"))
                        .and_then(|(_, _, v)| {
                            self.file.resolve_msl(self.method.offset, *v as u32).ok()
                        })
                        .unwrap_or(0)
                });
                let cls = Expr::ClassLit(off);
                if let Some(&v) = vs.first() {
                    self.set_v(v, cls.clone());
                }
                // defineclasswithbuffer 同时把类留在 acc（后续 sta）
                self.set_acc(cls);
            }
            // ---- 函数定义 / 字面量 ----
            "definefunc" => {
                let v = self.reg_of(i);
                let off = self.resolve_method_name(i).map(|(o, _)| o).unwrap_or(0);
                self.set_v(v, Expr::Closure { method_off: off });
                let e = self.v(v);
                self.stmts.push(Stmt::Expr(e));
            }
            "definefuncwithname" => {
                let v = self.reg_of(i);
                let off = self.resolve_method_name(i).map(|(o, _)| o).unwrap_or(0);
                self.set_v(v, Expr::Closure { method_off: off });
                let e = self.v(v);
                self.stmts.push(Stmt::Expr(e));
            }
            "definegeneratorfunc" | "defineasyncfunc" | "defineasynccGeneratorfunc" => {
                let v = self.reg_of(i);
                let off = self.resolve_method_name(i).map(|(o, _)| o).unwrap_or(0);
                self.set_v(v, Expr::Closure { method_off: off });
                let e = self.v(v);
                self.stmts.push(Stmt::Expr(e));
            }
            "createobjectwithbuffer" => {
                let off = self.litarr_of(i).unwrap_or(0);
                self.set_acc(Expr::ObjectLit(off));
            }
            "createarraywithbuffer" => {
                let off = self.litarr_of(i).unwrap_or(0);
                self.set_acc(Expr::ArrayLit(off));
            }
            "createemptyobject" => {
                let v = self.reg_of(i);
                let lit = Expr::TemplateLit("{}".into());
                self.set_v(v, lit.clone());
                self.stmts.push(Stmt::Expr(lit));
            }
            // ---- 词法环境 ----
            "ldlexvar" | "deprecated.ldlexvar" | "ldlexvar2" | "wide.ldlexvar" => {
                // acc = lexenv[imm1][imm2]，无寄存器操作数
                let (depth, slot) = two_imms(i);
                let named = self.lex_slot_name(depth as usize, slot as usize);
                match named {
                    Some(name) => self.set_acc(Expr::Name(name)),
                    None => self.set_acc(Expr::LexVar(depth as u32, slot as u32)),
                }
            }

            "stlexvar" | "deprecated.stlexvar" => {
                let (depth, slot) = two_imms(i);
                let v = self.reg_of(i);
                let val = self.peek_acc();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::LexVar(depth as u32, slot as u32)),
                    Box::new(val),
                )));
                let _ = v;
                self.acc_persisted = true;
            }
            "newlexenv" | "deprecated.newlexenv" | "newlexenvwithname" => {
                let v = self.reg_of(i);
                self.set_v(v, Expr::TemplateLit("/*lexenv*/{}".into()));
                let e = self.v(v);
                self.stmts.push(Stmt::Expr(e));
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
                let val = self.peek_acc();
                let name = self.module_export_name(idx);
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Name(format!("export:{name}"))),
                    Box::new(val),
                )));
                self.acc_persisted = true;
            }
            "getmodulenamespace" => {
                let v = self.reg_of(i);
                self.set_v(v, Expr::Special("import namespace"));
                let e = self.v(v);
                self.stmts.push(Stmt::Expr(e));
            }
            "ldexternalmodulevar" => {
                // acc = 本 record 模块导入表中的绑定（regular/namespace import）
                let idx = self.imm(i);
                self.set_acc(Expr::Name(self.module_import_name(idx)));
            }
            // ---- 变参 / apply / spread ----
            "copyrestargs" => {
                // 剩余参数数组进 acc（无寄存器操作数），渲染为 args / ...args
                self.set_acc(Expr::Name("args".into()));
            }
            "createemptyarray" | "createarray" => {
                // 数组进 acc（无寄存器操作数），构建器在 stownbyindex 惰性建立
                self.set_acc(Expr::Special("[]"));
            }
            "stownbyindex" => {
                // 数组[v][imm2] = acc
                let vs = self.regs_of(i);
                let arr = vs.first().copied().unwrap_or(0);
                let idx = self.imm_last(i) as usize;
                let val = self.peek_acc();
                let elems = self.array_builders.entry(arr).or_default();
                {
                    while elems.len() < idx {
                        elems.push(Expr::Special("undefined"));
                    }
                    if idx == elems.len() {
                        elems.push(val);
                    } else {
                        elems[idx] = val;
                    }
                }
            }
            "starrayspread" => {
                // 数组[v1] 追加 ...acc（v2 = 起始下标）
                let vs = self.regs_of(i);
                let arr = vs.first().copied().unwrap_or(0);
                let val = self.peek_acc();
                self.array_builders
                    .entry(arr)
                    .or_default()
                    .push(Expr::Spread(Box::new(val)));
            }
            "apply" => {
                // acc = fn；v1 = this；v2 = 参数数组（构建器可展开）
                let vs = self.regs_of(i);
                let this_reg = vs.first().copied().unwrap_or(0);
                let arr_reg = vs.get(1).copied().unwrap_or(0);
                let callee = self.take_acc();
                let args = match self.array_builders.remove(&arr_reg) {
                    Some(elems) if !elems.is_empty() => elems,
                    _ => vec![self.v(arr_reg)],
                };
                self.set_acc(Expr::Call {
                    callee: Box::new(callee),
                    args,
                    new: false,
                    this_arg: Some(Box::new(self.v(this_reg))),
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
                let val = self.peek_acc();
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
                // 丢弃 acc 前先补发副作用（未消费的调用）
                let dropped = self.take_acc();
                if has_effect(&dropped) && !matches!(dropped, Expr::Special(_)) {
                    self.stmts.push(Stmt::Expr(dropped));
                }
                self.stmts.push(Stmt::Return(None));
            }
            "throw" => {
                let e = self.take_acc();
                self.stmts.push(Stmt::Throw(e));
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
            "getiterator" | "getasynciterator" => {
                let recv = self.take_acc();
                self.set_acc(Expr::Call {
                    callee: Box::new(Expr::Name("/*iterator*/".into())),
                    args: vec![recv],
                    new: false,
                    this_arg: None,
                });
            }
            "getpropiterator" => {
                let recv = self.take_acc();
                self.set_acc(Expr::Call {
                    callee: Box::new(Expr::Name("Object.keys".into())),
                    args: vec![recv],
                    new: false,
                    this_arg: None,
                });
            }
            // ---- 运行时调用 ----
            other => {
                if let Some(rest) = other.strip_prefix("callruntime.") {
                    self.lift_callruntime(rest, i);
                } else if other.starts_with("throw.") {
                    // 编译器生成的 TDZ/参数检查（throw.undefinedifholewithname 等）
                    // 在源码中不存在，跳过
                    if !other.contains("ifhole") {
                        let name = self.resolve_str(i);
                        let what = if name.is_empty() {
                            other.to_string()
                        } else {
                            format!("{other} {name:?}")
                        };
                        self.stmts.push(Stmt::Comment(what));
                    }
                } else if other.starts_with("wide.") {
                    // 宽形式语义与非宽一致，直接按基名处理
                    let base = other.trim_start_matches("wide.");
                    self.visit_named(i, base);
                } else {
                    self.fallback(i);
                }
            }
        }
    }

    fn visit_named(&mut self, i: &Ins, base: &str) {
        // 复用主 match：构造同义分发（当前覆盖常用子集）
        match base {
            "add2" => self.bin("+", i),
            "sub2" => self.bin("-", i),
            "mul2" => self.bin("*", i),
            "div2" => self.bin("/", i),
            "ldobjbyname" => {
                // acc = acc.<name>（无寄存器操作数）
                let name = self.resolve_str(i);
                let obj = self.take_acc();
                self.set_acc(Expr::Member(Box::new(obj), name));
            }
            "ldai" => {
                let v = signed_imm_of(i);
                self.set_acc(Expr::Int(v));
            }
            "lda.str" => self.set_acc(Expr::Str(self.resolve_str(i))),
            _ => self.fallback(i),
        }
    }

    fn lift_callruntime(&mut self, name: &str, i: &Ins) {
        match name {
            "definefieldbyvalue" | "definefieldbyindex" | "definefieldsbyname" => {
                let val = self.take_acc();
                self.stmts.push(Stmt::Expr(Expr::Assign(
                    Box::new(Expr::Member(
                        Box::new(Expr::Special("target")),
                        format!("/*{name}*/"),
                    )),
                    Box::new(val),
                )));
            }
            "definefunc" | "definemethod" | "defineclasswithbuffer" | "definesendableclass" => {
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
            "createprivateproperty"
            | "ldprivateproperty"
            | "stprivateproperty"
            | "calli0"
            | "calli1"
            | "newlexenv"
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

    fn bin(&mut self, op: &'static str, i: &Ins) {
        // es2abc 计算顺序：RHS 先入 acc，左操作数在 v 寄存器 → acc = v OP acc
        let v = self.reg_of(i);
        let rhs = self.take_acc();
        let lhs = self.v(v);
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

/// 表达式是否含调用副作用（被丢弃时也要发出语句）
pub fn has_effect(e: &Expr) -> bool {
    match e {
        // 调用本身就是副作用
        Expr::Call { .. } | Expr::Update(_, _, _) | Expr::Assign(_, _) => true,
        Expr::Bin(_, a, b) | Expr::Index(a, b) => has_effect(a) || has_effect(b),
        Expr::Un(_, a) => has_effect(a),
        Expr::Member(o, _) => has_effect(o),
        Expr::Cond(c, a, b) => has_effect(c) || has_effect(a) || has_effect(b),
        Expr::Spread(a) => has_effect(a),
        _ => false,
    }
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
        if n == "imm" {
            vals.push(v);
        }
    }
    (
        vals.first().copied().unwrap_or(0),
        vals.get(1).copied().unwrap_or(0),
    )
}

/// 条件取反（jeqz 语义）
fn invert_cond(e: Expr) -> Expr {
    Expr::Un("!", Box::new(e))
}
