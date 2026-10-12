//! 结构化后处理：比较链 → switch 还原
//!
//! 参考《A Jump-Table-Agnostic Switch Recovery on ASTs》的 T1 规则：
//! `if (α === c1) … else if (α === c2) …` 的同 α 常量比较链改写为 switch。
//! Panda 字节码没有 table-switch，`jeqz + stricteq2 imm` 链是唯一来源，
//! 因此这里只做同变量常量等值链的保守匹配。
//!
//! @author ljlVink

use crate::decompile::lift::{Expr, Stmt};

/// 尝试把语句序列中的 if-else-if 常量链改写为 switch
/// （当前保守实现：识别链并在注释中标出，不改写语义）
pub fn recover_switches(stmts: &mut [Stmt]) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::If(_, then, els) => {
                recover_switches(then);
                recover_switches(els);
            }
            Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::ForOf(_, _, body) => {
                recover_switches(body)
            }
            Stmt::Try(body, catches) => {
                recover_switches(body);
                for (_, h) in catches {
                    recover_switches(h);
                }
            }
            _ => {}
        }
    }
}

/// 语句包含等值子树（含条件）
fn stmt_contains_eq(s: &Stmt, val: &Expr, budget: &mut i32) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => contains_eq(e, val, budget),
        Stmt::Declare(_, Some(e)) => contains_eq(e, val, budget),
        Stmt::If(c, _, _) => contains_eq(c, val, budget),
        Stmt::While(c, _) => contains_eq(c, val, budget),
        _ => false,
    }
}

fn val_of(s: &Stmt) -> &Expr {
    match s {
        Stmt::Expr(Expr::Assign(_, v)) => v,
        _ => unreachable!(),
    }
}

/// 表达式树中是否包含与 val 结构相等的子树（有界）
fn contains_eq(e: &Expr, val: &Expr, budget: &mut i32) -> bool {
    if *budget <= 0 {
        return false;
    }
    *budget -= 1;
    let mut b = 512i32;
    if crate::decompile::printer::expr_eq_bounded(e, val, &mut b) {
        return true;
    }
    match e {
        Expr::Bin(_, a, b) | Expr::Index(a, b) => {
            contains_eq(a, val, budget) || contains_eq(b, val, budget)
        }
        Expr::Un(_, a) | Expr::Update(_, a, _) | Expr::Member(a, _) | Expr::Assign(_, a) => {
            contains_eq(a, val, budget)
        }
        Expr::Cond(c, a, b) => {
            contains_eq(c, val, budget)
                || contains_eq(a, val, budget)
                || contains_eq(b, val, budget)
        }
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            contains_eq(callee, val, budget)
                || args.iter().any(|x| contains_eq(x, val, budget))
                || this_arg
                    .as_ref()
                    .is_some_and(|t| contains_eq(t, val, budget))
        }
        _ => false,
    }
}

/// 不可达语句截断：同层 return 之后的语句是线性拼块残渣，删除。
/// 递归处理控制流体内。
pub fn truncate_after_return(stmts: &mut Vec<Stmt>) {
    if let Some(pos) = stmts.iter().position(|s| matches!(s, Stmt::Return(_))) {
        stmts.truncate(pos + 1);
    }
    for s in stmts.iter_mut() {
        match s {
            Stmt::If(_, then, els) => {
                truncate_after_return(then);
                truncate_after_return(els);
            }
            Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::ForOf(_, _, body) => {
                truncate_after_return(body)
            }
            Stmt::Try(body, catches) => {
                truncate_after_return(body);
                for (_, h) in catches {
                    truncate_after_return(h);
                }
            }
            _ => {}
        }
    }
}

/// 死赋值消除：删除 `vN = expr`（无副作用且 vN 在方法内无其他引用）。
/// 寄存器值在使用点已被内联，这类赋值是提升阶段的残渣。
pub fn eliminate_dead_stores(stmts: &mut Vec<Stmt>) {
    // 孤儿表达式：纯读取（无副作用、无赋值）的语句是提升残渣，直接删
    stmts.retain(|s| match s {
        Stmt::Expr(e) => crate::decompile::lift::has_effect(e),
        _ => true,
    });

    let mut i = 0;
    while i < stmts.len() {
        let dead = match &stmts[i] {
            Stmt::Expr(Expr::Assign(target, val)) => match target.as_ref() {
                Expr::Reg(n) => {
                    let n = *n;
                    if !contains_reg(val, n)
                        && !stmts
                            .iter()
                            .enumerate()
                            .any(|(j, s)| j != i && count_reg_uses(s, n) > 0)
                    {
                        // 无副作用：直接删
                        !crate::decompile::lift::has_effect(val)
                    } else {
                        false
                    }
                }
                _ => false,
            },
            _ => false,
        };
        // 副作用存储：值在紧接着的语句中完整复现 → 冗余（寄存器值已内联到使用点）
        let redundant = !dead
            && matches!(
                &stmts[i],
                Stmt::Expr(Expr::Assign(t, _)) if matches!(t.as_ref(), Expr::Reg(_)))
            && stmts.get(i + 1).is_some_and(|next| {
                let mut budget = 2048i32;
                stmt_contains_eq(next, val_of(&stmts[i]), &mut budget)
            });
        if dead || redundant {
            stmts.remove(i);
        } else {
            i += 1;
        }
    }
    // 递归处理嵌套体
    for s in stmts.iter_mut() {
        match s {
            Stmt::If(_, then, els) => {
                eliminate_dead_stores(then);
                eliminate_dead_stores(els);
            }
            Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::ForOf(_, _, body) => {
                eliminate_dead_stores(body)
            }
            Stmt::Try(body, catches) => {
                eliminate_dead_stores(body);
                for (_, h) in catches {
                    eliminate_dead_stores(h);
                }
            }
            _ => {}
        }
    }
}

/// 表达式节点数（深克隆成本上界）
pub(crate) fn count_nodes(e: &Expr) -> usize {
    match e {
        Expr::Bin(_, a, b) => 1 + count_nodes(a) + count_nodes(b),
        Expr::Un(_, a) | Expr::Update(_, a, _) | Expr::Member(a, _) | Expr::Index(a, _) => {
            1 + count_nodes(a)
        }
        Expr::Assign(t, v) => 1 + count_nodes(t) + count_nodes(v),
        Expr::Cond(c, a, b) => 1 + count_nodes(c) + count_nodes(a) + count_nodes(b),
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            1 + count_nodes(callee)
                + args.iter().map(count_nodes).sum::<usize>()
                + this_arg.as_ref().map(|t| count_nodes(t)).unwrap_or(0)
        }
        _ => 1,
    }
}

/// 单用寄存器临时变量折叠：`vN = EXPR; …; f(vN)` → `f(EXPR)`
/// 定义语句在窗口（4 条）内恰好被引用一次且之后不再引用时折叠。
/// 带节点预算：折叠是深克隆，链式折叠会让树体积乘性增长，
/// 超预算即停止，防止内存/CPU 失控。
pub fn fold_register_stmts(stmts: &mut Vec<Stmt>) {
    const MAX_VAL_NODES: usize = 200;
    const MAX_TOTAL_NODES: usize = 100_000;
    let mut budget = MAX_TOTAL_NODES;
    let mut i = 0;
    'outer: while i < stmts.len() {
        let target = match &stmts[i] {
            Stmt::Expr(crate::decompile::lift::Expr::Assign(target, val)) => {
                match target.as_ref() {
                    crate::decompile::lift::Expr::Reg(n)
                        if !crate::decompile::lift::has_effect(val) =>
                    {
                        Some((*n, val.clone()))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some((reg, val)) = target {
            if contains_reg(&val, reg) {
                i += 1;
                continue;
            }
            let val_nodes = count_nodes(&val);
            if val_nodes > MAX_VAL_NODES || budget < val_nodes {
                i += 1;
                continue;
            }
            // 定位第一条读 reg 的语句（上限 12 条防表达式膨胀）：
            // 折入链头（(v2 = v2 + x) 的 RHS），链在后续轮次滚动折叠，
            // 直至终点使用自然拿到完整表达式。
            // - 不读 reg 的语句直接跳过
            // - 读 1 次 → 折入；读多次（分叉）放弃
            let window = stmts.len().min(i + 12);
            let mut use_j = None;
            for (j, stmt) in stmts.iter().enumerate().take(window).skip(i + 1) {
                let reads = count_reg_reads(stmt, reg);
                if reads == 0 {
                    continue;
                }
                if reads == 1 {
                    use_j = Some(j);
                }
                break;
            }
            if let Some(j) = use_j {
                // kill 集：val 引用的寄存器在 def..use 区间被重定义则不折
                // （a/b 交换的 (v3 = v0) … (v0 = v1) … (v1 = v3) 中 v0 被杀）
                let mut live_regs = Vec::new();
                collect_expr_regs(&val, &mut live_regs);
                let killed = stmts[i + 1..j].iter().any(|s| {
                    live_regs
                        .iter()
                        .any(|r| is_reg_def(s, *r) || count_reg_reads(s, *r) > 0)
                });
                if !killed {
                    let mut next = stmts[j].clone();
                    substitute_reg(&mut next, reg, &val);
                    budget = budget.saturating_sub(val_nodes);
                    stmts[j] = next;
                    stmts.remove(i);
                    continue 'outer;
                }
            }
        }
        i += 1;
    }
}

/// 收集表达式引用的全部寄存器
fn collect_expr_regs(e: &Expr, out: &mut Vec<u32>) {
    match e {
        Expr::Reg(n) => {
            if !out.contains(n) {
                out.push(*n);
            }
        }
        Expr::Bin(_, a, b) | Expr::Assign(a, b) => {
            collect_expr_regs(a, out);
            collect_expr_regs(b, out);
        }
        Expr::Un(_, a) | Expr::Member(a, _) | Expr::Update(_, a, _) => {
            collect_expr_regs(a, out);
        }
        Expr::Index(a, b) => {
            collect_expr_regs(a, out);
            collect_expr_regs(b, out);
        }
        Expr::Cond(a, b, c) => {
            collect_expr_regs(a, out);
            collect_expr_regs(b, out);
            collect_expr_regs(c, out);
        }
        _ => {}
    }
}

/// 语句是否给寄存器 reg 赋值（定义点）
fn is_reg_def(s: &Stmt, reg: u32) -> bool {
    matches!(
        s,
        Stmt::Expr(Expr::Assign(t, _)) if matches!(t.as_ref(), Expr::Reg(n) if *n == reg))
}

/// 语句对寄存器的"读"计数（Assign 的 target 是定义不是读）
fn count_reg_reads(s: &Stmt, reg: u32) -> usize {
    let mut c = 0;
    match s {
        Stmt::Expr(Expr::Assign(t, v)) => {
            // target 为其它寄存器时不算读；target 恰为 reg 时是重定义
            let _ = t;
            c += count_expr_reg_uses(v, reg);
        }
        _ => visit_stmt_exprs(s, &mut |e| c += count_expr_reg_uses(e, reg)),
    }
    c
}

fn count_reg_uses(s: &Stmt, reg: u32) -> usize {
    let mut c = 0;
    visit_stmt_exprs(s, &mut |e| c += count_expr_reg_uses(e, reg));
    c
}

fn contains_reg(e: &crate::decompile::lift::Expr, reg: u32) -> bool {
    count_expr_reg_uses(e, reg) > 0
}

fn count_expr_reg_uses(e: &crate::decompile::lift::Expr, reg: u32) -> usize {
    use crate::decompile::lift::Expr;
    match e {
        Expr::Reg(n) if *n == reg => 1,
        Expr::Assign(a, b) => {
            // 赋值左侧裸寄存器是定义点，不算使用
            match a.as_ref() {
                Expr::Reg(_) => count_expr_reg_uses(b, reg),
                _ => count_expr_reg_uses(a, reg) + count_expr_reg_uses(b, reg),
            }
        }
        Expr::Bin(_, a, b) | Expr::Index(a, b) => {
            count_expr_reg_uses(a, reg) + count_expr_reg_uses(b, reg)
        }
        Expr::Un(_, a) | Expr::Update(_, a, _) | Expr::Member(a, _) => count_expr_reg_uses(a, reg),
        Expr::Cond(c, a, b) => {
            count_expr_reg_uses(c, reg) + count_expr_reg_uses(a, reg) + count_expr_reg_uses(b, reg)
        }
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            count_expr_reg_uses(callee, reg)
                + args
                    .iter()
                    .map(|a| count_expr_reg_uses(a, reg))
                    .sum::<usize>()
                + this_arg
                    .as_ref()
                    .map(|t| count_expr_reg_uses(t, reg))
                    .unwrap_or(0)
        }
        _ => 0,
    }
}

fn substitute_reg(s: &mut Stmt, reg: u32, val: &crate::decompile::lift::Expr) {
    visit_stmt_exprs_mut(s, &mut |e| substitute_expr_reg(e, reg, val));
}

fn substitute_expr_reg(
    e: &mut crate::decompile::lift::Expr,
    reg: u32,
    val: &crate::decompile::lift::Expr,
) {
    use crate::decompile::lift::Expr;
    match e {
        Expr::Reg(n) if *n == reg => *e = val.clone(),
        Expr::Assign(a, b) => match a.as_ref() {
            Expr::Reg(_) => substitute_expr_reg(b, reg, val),
            _ => {
                substitute_expr_reg(a, reg, val);
                substitute_expr_reg(b, reg, val);
            }
        },
        Expr::Bin(_, a, b) | Expr::Index(a, b) => {
            substitute_expr_reg(a, reg, val);
            substitute_expr_reg(b, reg, val);
        }
        Expr::Un(_, a) | Expr::Update(_, a, _) | Expr::Member(a, _) => {
            substitute_expr_reg(a, reg, val)
        }
        Expr::Cond(c, a, b) => {
            substitute_expr_reg(c, reg, val);
            substitute_expr_reg(a, reg, val);
            substitute_expr_reg(b, reg, val);
        }
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            substitute_expr_reg(callee, reg, val);
            for a in args {
                substitute_expr_reg(a, reg, val);
            }
            if let Some(t) = this_arg {
                substitute_expr_reg(t, reg, val);
            }
        }
        _ => {}
    }
}

fn visit_stmt_exprs(s: &Stmt, f: &mut dyn FnMut(&crate::decompile::lift::Expr)) {
    use crate::decompile::lift::Stmt;
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => f(e),
        Stmt::Declare(_, Some(e)) => f(e),
        Stmt::If(c, t, e) => {
            f(c);
            for s in t.iter().chain(e.iter()) {
                visit_stmt_exprs(s, f);
            }
        }
        Stmt::While(c, b) | Stmt::DoWhile(b, c) => {
            f(c);
            for s in b {
                visit_stmt_exprs(s, f);
            }
        }
        Stmt::ForOf(_, i, b) => {
            f(i);
            for s in b {
                visit_stmt_exprs(s, f);
            }
        }
        Stmt::Try(b, catches) => {
            for s in b {
                visit_stmt_exprs(s, f);
            }
            for (_, h) in catches {
                for s in h {
                    visit_stmt_exprs(s, f);
                }
            }
        }
        _ => {}
    }
}

fn visit_stmt_exprs_mut(s: &mut Stmt, f: &mut dyn FnMut(&mut crate::decompile::lift::Expr)) {
    use crate::decompile::lift::Stmt;
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => f(e),
        Stmt::Declare(_, Some(e)) => f(e),
        Stmt::If(c, t, e) => {
            f(c);
            for s in t.iter_mut().chain(e.iter_mut()) {
                visit_stmt_exprs_mut(s, f);
            }
        }
        Stmt::While(c, b) => {
            f(c);
            for s in b.iter_mut() {
                visit_stmt_exprs_mut(s, f);
            }
        }
        Stmt::DoWhile(b, c) => {
            f(c);
            for s in b.iter_mut() {
                visit_stmt_exprs_mut(s, f);
            }
        }
        Stmt::ForOf(_, i, b) => {
            f(i);
            for s in b.iter_mut() {
                visit_stmt_exprs_mut(s, f);
            }
        }
        Stmt::Try(b, catches) => {
            for s in b.iter_mut() {
                visit_stmt_exprs_mut(s, f);
            }
            for (_, h) in catches.iter_mut() {
                for s in h.iter_mut() {
                    visit_stmt_exprs_mut(s, f);
                }
            }
        }
        _ => {}
    }
}

/// 判断条件是否为 `expr === 常量` 形式
pub fn is_const_eq(e: &Expr) -> bool {
    matches!(e, Expr::Bin("===" | "==", _, b) if matches!(**b, Expr::Int(_) | Expr::Str(_)))
}
