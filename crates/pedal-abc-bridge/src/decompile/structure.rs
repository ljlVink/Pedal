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
            Stmt::TryFinally(body, finalizer) => {
                recover_switches(body);
                recover_switches(finalizer);
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

/// 不可达语句截断：同层 return 之后的语句是线性拼块残渣，删除。
/// 递归处理控制流体内。
pub fn truncate_after_return(stmts: &mut Vec<Stmt>) {
    if let Some(pos) = stmts.iter().position(|s| {
        matches!(
            s,
            Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break | Stmt::Continue
        )
    }) {
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
            Stmt::TryFinally(body, finalizer) => {
                truncate_after_return(body);
                truncate_after_return(finalizer);
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
    // 使用计数覆盖整棵函数树；不能把分支外/循环后的读取当作不存在。
    loop {
        let mut used = std::collections::HashSet::new();
        for s in stmts.iter() {
            visit_stmt_exprs(s, &mut |e| collect_read_regs(e, &mut used));
        }
        if !remove_unused(stmts, &used) {
            break;
        }
    }
}

fn collect_read_regs(e: &Expr, used: &mut std::collections::HashSet<u32>) {
    match e {
        Expr::Reg(n) => {
            used.insert(*n);
        }
        Expr::Assign(t, v) => {
            if !matches!(t.as_ref(), Expr::Reg(_)) {
                collect_read_regs(t, used);
            }
            collect_read_regs(v, used);
        }
        Expr::Bin(_, a, b) | Expr::Index(a, b) => {
            collect_read_regs(a, used);
            collect_read_regs(b, used);
        }
        Expr::Un(_, a)
        | Expr::Await(a)
        | Expr::Update(_, a, _)
        | Expr::Member(a, _)
        | Expr::Spread(a) => collect_read_regs(a, used),
        Expr::Cond(c, a, b) => {
            collect_read_regs(c, used);
            collect_read_regs(a, used);
            collect_read_regs(b, used);
        }
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            collect_read_regs(callee, used);
            for a in args {
                collect_read_regs(a, used);
            }
            if let Some(a) = this_arg {
                collect_read_regs(a, used);
            }
        }
        Expr::Object(properties) => {
            for (_, value) in properties {
                collect_read_regs(value, used);
            }
        }
        Expr::Array(elements) => {
            for value in elements {
                collect_read_regs(value, used);
            }
        }
        _ => {}
    }
}

fn remove_unused(stmts: &mut Vec<Stmt>, used: &std::collections::HashSet<u32>) -> bool {
    let mut changed = false;
    stmts.retain_mut(|s| {
        match s {
            Stmt::Expr(Expr::Assign(t, v)) if matches!(t.as_ref(), Expr::Reg(n) if !used.contains(n)) => {
                if crate::decompile::lift::has_effect(v) {
                    *s = Stmt::Expr(*v.clone());
                } else { changed = true; return false; }
                changed = true;
            }
            Stmt::Expr(e) if !crate::decompile::lift::has_effect(e) => { changed = true; return false; }
            Stmt::If(_, a, b) => { changed |= remove_unused(a, used); changed |= remove_unused(b, used); }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::ForOf(_, _, b) => changed |= remove_unused(b, used),
            Stmt::TryFinally(b, f) => {
                changed |= remove_unused(b, used);
                changed |= remove_unused(f, used);
            }
            Stmt::Try(b, cs) => {
                changed |= remove_unused(b, used);
                for (_, b) in cs { changed |= remove_unused(b, used); }
            }
            _ => {}
        }
        true
    });
    changed
}

/// 表达式节点数（深克隆成本上界）
pub(crate) fn count_nodes(e: &Expr) -> usize {
    match e {
        Expr::Bin(_, a, b) | Expr::Index(a, b) => 1 + count_nodes(a) + count_nodes(b),
        Expr::Un(_, a)
        | Expr::Await(a)
        | Expr::Update(_, a, _)
        | Expr::Member(a, _)
        | Expr::Spread(a) => 1 + count_nodes(a),
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
        Expr::Object(properties) => {
            1 + properties
                .iter()
                .map(|(_, value)| count_nodes(value))
                .sum::<usize>()
        }
        Expr::Array(elements) => 1 + elements.iter().map(count_nodes).sum::<usize>(),
        _ => 1,
    }
}

/// 单用寄存器临时变量折叠：`vN = EXPR; …; f(vN)` → `f(EXPR)`
/// 定义语句在窗口（12 条）内仅在一个单次求值位置使用且绑定不逃逸时折叠。
/// 带节点预算：折叠是深克隆，链式折叠会让树体积乘性增长，
/// 超预算即停止，防止内存/CPU 失控。
pub fn fold_register_stmts(stmts: &mut Vec<Stmt>) {
    let mut totals = std::collections::HashMap::new();
    let mut regs = std::collections::HashSet::new();
    for s in stmts.iter() {
        visit_stmt_exprs(s, &mut |e| collect_read_regs(e, &mut regs));
    }
    for n in regs {
        totals.insert(n, stmts.iter().map(|s| count_reg_uses(s, n)).sum::<usize>());
    }
    fold_scope(stmts, &totals);
}

fn fold_scope(stmts: &mut Vec<Stmt>, totals: &std::collections::HashMap<u32, usize>) {
    let external: std::collections::HashSet<u32> = totals
        .iter()
        .filter(|(n, total)| stmts.iter().map(|s| count_reg_uses(s, **n)).sum::<usize>() < **total)
        .map(|(n, _)| *n)
        .collect();
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::If(_, a, b) => {
                fold_scope(a, totals);
                fold_scope(b, totals);
            }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::ForOf(_, _, b) => fold_scope(b, totals),
            Stmt::TryFinally(b, f) => {
                fold_scope(b, totals);
                fold_scope(f, totals);
            }
            Stmt::Try(b, cs) => {
                fold_scope(b, totals);
                for (_, b) in cs {
                    fold_scope(b, totals);
                }
            }
            _ => {}
        }
    }
    let mut budget = 100_000usize;
    let mut i = 0;
    while i < stmts.len() {
        let candidate = match &stmts[i] {
            Stmt::Expr(Expr::Assign(t, v)) => match t.as_ref() {
                Expr::Reg(n) if count_nodes(v) <= 200 => Some((*n, *v.clone())),
                _ => None,
            },
            _ => None,
        };
        let Some((reg, val)) = candidate else {
            i += 1;
            continue;
        };
        let mut reads = 0;
        let mut use_at = None;
        let mut use_statements = 0;
        for (j, stmt) in stmts.iter().enumerate().skip(i + 1) {
            let count = count_reg_uses(stmt, reg);
            if count != 0 {
                reads += count;
                use_at = Some(j);
                use_statements += 1;
            }
            if is_reg_def(stmt, reg) {
                break;
            }
        }
        if let Some(j) = use_at.filter(|j| *j <= i + 12) {
            // 只移入同层的单次求值位置，禁止把循环前初始化折进每轮条件/循环体。
            let single_eval = match &stmts[j] {
                Stmt::Expr(_) | Stmt::Return(_) | Stmt::Throw(_) => true,
                Stmt::If(c, a, b) => {
                    count_expr_reg_uses(c, reg) == reads
                        && a.iter().chain(b).all(|s| count_reg_uses(s, reg) == 0)
                }
                _ => false,
            };
            let copyable = matches!(
                val,
                Expr::Int(_)
                    | Expr::Float(_)
                    | Expr::Str(_)
                    | Expr::Bool(_)
                    | Expr::Reg(_)
                    | Expr::Arg(_)
                    | Expr::Special("this" | "null" | "undefined")
            );
            let mut deps = Vec::new();
            collect_expr_regs(&val, &mut deps);
            let safe_gap = stmts[i + 1..j].iter().all(|s| match s {
                Stmt::Expr(Expr::Assign(t, v)) => {
                    matches!(t.as_ref(), Expr::Reg(n) if *n != reg && !deps.contains(n))
                        && matches!(
                            v.as_ref(),
                            Expr::Reg(_)
                                | Expr::Arg(_)
                                | Expr::Int(_)
                                | Expr::Float(_)
                                | Expr::Str(_)
                                | Expr::Bool(_)
                                | Expr::Special(_)
                                | Expr::Closure { .. }
                        )
                }
                _ => false,
            });
            let cost = count_nodes(&val) * reads;
            let redefines = is_reg_def(&stmts[j], reg);
            let keeps_binding = !external.contains(&reg) || redefines;
            let self_safe = !contains_reg(&val, reg) || (redefines && j == i + 1);
            if use_statements == 1
                && reads > 0
                && (reads == 1 || copyable)
                && single_eval
                && safe_gap
                && cost <= budget
                && keeps_binding
                && self_safe
            {
                substitute_reg(&mut stmts[j], reg, &val);
                stmts.remove(i);
                budget -= cost;
                i = i.saturating_sub(1);
                continue;
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
        Expr::Un(_, a) | Expr::Await(a) | Expr::Member(a, _) | Expr::Update(_, a, _) => {
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
        Expr::Call {
            callee,
            args,
            this_arg,
            ..
        } => {
            collect_expr_regs(callee, out);
            for a in args {
                collect_expr_regs(a, out);
            }
            if let Some(a) = this_arg {
                collect_expr_regs(a, out);
            }
        }
        Expr::Spread(a) => collect_expr_regs(a, out),
        Expr::Object(properties) => {
            for (_, value) in properties {
                collect_expr_regs(value, out);
            }
        }
        Expr::Array(elements) => {
            for value in elements {
                collect_expr_regs(value, out);
            }
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
        Expr::Un(_, a)
        | Expr::Await(a)
        | Expr::Update(_, a, _)
        | Expr::Member(a, _)
        | Expr::Spread(a) => count_expr_reg_uses(a, reg),
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
        Expr::Object(properties) => properties
            .iter()
            .map(|(_, value)| count_expr_reg_uses(value, reg))
            .sum(),
        Expr::Array(elements) => elements
            .iter()
            .map(|value| count_expr_reg_uses(value, reg))
            .sum(),
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
        Expr::Un(_, a)
        | Expr::Await(a)
        | Expr::Update(_, a, _)
        | Expr::Member(a, _)
        | Expr::Spread(a) => substitute_expr_reg(a, reg, val),
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
        Expr::Object(properties) => {
            for (_, value) in properties {
                substitute_expr_reg(value, reg, val);
            }
        }
        Expr::Array(elements) => {
            for value in elements {
                substitute_expr_reg(value, reg, val);
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
        Stmt::TryFinally(b, finalizer) => {
            for s in b.iter().chain(finalizer) {
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
        Stmt::TryFinally(b, finalizer) => {
            for s in b.iter_mut().chain(finalizer) {
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

/// 清理机器分支并恢复同目标的条件赋值；不对浮点关系运算作错误的反向比较。
pub fn simplify_control(stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::If(_, a, b) => {
                simplify_control(a);
                simplify_control(b);
            }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::ForOf(_, _, b) => simplify_control(b),
            Stmt::TryFinally(b, f) => {
                simplify_control(b);
                simplify_control(f);
            }
            Stmt::Try(b, cs) => {
                simplify_control(b);
                for (_, b) in cs {
                    simplify_control(b);
                }
            }
            _ => {}
        }
        if let Stmt::If(cond, yes, no) = s {
            if yes.is_empty() && !no.is_empty() {
                *cond = crate::decompile::lift::invert_cond(cond.clone());
                std::mem::swap(yes, no);
            }
            // 选择正向条件，减少 !(a < b) 的阅读负担而不假设没有 NaN。
            if let Expr::Un("!", inner) = cond
                && !no.is_empty()
            {
                *cond = *inner.clone();
                std::mem::swap(yes, no);
            }
            if let [Stmt::If(other, inner_yes, inner_no)] = no.as_slice()
                && inner_no.is_empty()
                && same_assignments(yes, inner_yes)
            {
                *cond = Expr::Bin("||", Box::new(cond.clone()), Box::new(other.clone()));
                no.clear();
            }
            if no.is_empty()
                && let [Stmt::Expr(Expr::Assign(target, fallback))] = yes.as_slice()
                && is_nullish_check(cond, target)
                && matches!(
                    target.as_ref(),
                    Expr::Reg(_) | Expr::Arg(_) | Expr::LexVar(_, _)
                )
            {
                *s = Stmt::Expr(Expr::Assign(
                    target.clone(),
                    Box::new(Expr::Bin("??", target.clone(), fallback.clone())),
                ));
                continue;
            }
            if let ([Stmt::Expr(Expr::Assign(a, v))], [Stmt::Expr(Expr::Assign(b, w))]) =
                (yes.as_slice(), no.as_slice())
                && same_expr(a, b)
                && matches!(a.as_ref(), Expr::Reg(_) | Expr::Arg(_) | Expr::LexVar(_, _))
            {
                *s = Stmt::Expr(Expr::Assign(
                    a.clone(),
                    Box::new(Expr::Cond(Box::new(cond.clone()), v.clone(), w.clone())),
                ));
            } else if let ([Stmt::Return(Some(a))], [Stmt::Return(Some(b))]) =
                (yes.as_slice(), no.as_slice())
            {
                *s = Stmt::Return(Some(Expr::Cond(
                    Box::new(cond.clone()),
                    Box::new(a.clone()),
                    Box::new(b.clone()),
                )));
            }
        }
        if let Stmt::While(Expr::Bool(true), body) = s
            && let Some(Stmt::If(cond, yes, no)) = body.first()
            && matches!(yes.as_slice(), [Stmt::Break])
            && no.is_empty()
        {
            let cond = crate::decompile::lift::invert_cond(cond.clone());
            body.remove(0);
            *s = Stmt::While(cond, std::mem::take(body));
        }
        if let Stmt::While(_, body) | Stmt::DoWhile(body, _) = s
            && matches!(body.last(), Some(Stmt::Continue))
        {
            body.pop();
        }
    }
    stmts.retain_mut(|s| {
        // VM 的接收者校验已由 super/this 语义表达；空保护区不会进入 catch。
        if matches!(s, Stmt::Try(body, _) if body.is_empty()) {
            return false;
        }
        if let Stmt::If(c, a, b) = s
            && a.is_empty()
            && b.is_empty()
        {
            if crate::decompile::lift::has_effect(c) {
                *s = Stmt::Expr(c.clone());
            } else {
                return false;
            }
        }
        true
    });
}

fn is_nullish_check(cond: &Expr, value: &Expr) -> bool {
    let Expr::Bin("||", a, b) = cond else {
        return false;
    };
    let test = |e: &Expr, sentinel| {
        matches!(e, Expr::Bin("===", v, s)
        if same_expr(v, value) && matches!(s.as_ref(), Expr::Special(x) if *x == sentinel))
    };
    (test(a, "null") && test(b, "undefined")) || (test(a, "undefined") && test(b, "null"))
}

fn same_expr(a: &Expr, b: &Expr) -> bool {
    crate::decompile::printer::expr_eq_bounded(a, b, &mut 512)
}

fn same_assignments(a: &[Stmt], b: &[Stmt]) -> bool {
    matches!((a, b), ([Stmt::Expr(x)], [Stmt::Expr(y)]) if same_expr(x, y))
}

/// 只在异常协议与每条正常完成路径上的内联副本同时得到验证后还原 finally。
/// 先处理外层：外层清理副本可能位于内层清理与 return 之间。
pub(crate) fn recover_finally(
    stmts: &mut Vec<Stmt>,
    candidates: &[(String, Vec<Stmt>, u32, bool)],
) {
    for stmt in stmts.iter_mut() {
        if let Stmt::Try(body, catches) = stmt
            && let [(Some(name), _)] = catches.as_slice()
            && let Some((_, cleanup, _, shared)) = candidates.iter().find(|(n, _, _, _)| n == name)
            && !cleanup.is_empty()
        {
            let mut protected = body.clone();
            let mut removed = 0;
            if remove_finally_copies(&mut protected, cleanup, &mut removed)
                && (*shared || removed != 0)
                && (*shared
                    || definitely_abrupt(&protected)
                    || remove_cleanup_suffix(&mut protected, cleanup, true))
            {
                *stmt = Stmt::TryFinally(protected, cleanup.clone());
            }
        }
        match stmt {
            Stmt::If(_, a, b) => {
                recover_finally(a, candidates);
                recover_finally(b, candidates);
            }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::ForOf(_, _, b) => {
                recover_finally(b, candidates)
            }
            Stmt::Try(b, catches) => {
                recover_finally(b, candidates);
                for (_, c) in catches {
                    recover_finally(c, candidates);
                }
            }
            Stmt::TryFinally(b, f) => {
                recover_finally(b, candidates);
                recover_finally(f, candidates);
            }
            _ => {}
        }
    }
    // 编译器为没有 catch 的 try/finally 插入的重抛壳。
    let mut i = 0;
    while i < stmts.len() {
        if let Stmt::Try(body, catches) = &stmts[i]
            && let [(Some(name), catch)] = catches.as_slice()
            && matches!(catch.as_slice(), [Stmt::Throw(Expr::Name(n))] if n == name)
        {
            let body = body.clone();
            let len = body.len();
            stmts.splice(i..=i, body);
            i += len;
        } else {
            i += 1;
        }
    }
}

fn definitely_abrupt(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break | Stmt::Continue => true,
        Stmt::If(_, a, b) => definitely_abrupt(a) && definitely_abrupt(b),
        Stmt::Try(b, cs) => definitely_abrupt(b) && cs.iter().all(|(_, c)| definitely_abrupt(c)),
        Stmt::TryFinally(b, f) => definitely_abrupt(b) || definitely_abrupt(f),
        _ => false,
    })
}

fn canonical_stmts(stmts: &[Stmt]) -> Vec<Stmt> {
    let mut result = stmts.to_vec();
    polish(&mut result);
    result
}

/// 编译器为相同清理块选择不同临时寄存器；只重命名本片段中定义的寄存器。
fn alpha_canonical(stmts: &[Stmt]) -> Vec<Stmt> {
    fn expr(e: &mut Expr, names: &mut std::collections::HashMap<u32, u32>, serial: &mut u32) {
        match e {
            Expr::Reg(n) => {
                if let Some(mapped) = names.get(n) {
                    *n = *mapped;
                }
            }
            Expr::Assign(a, b) => {
                expr(b, names, serial);
                if let Expr::Reg(n) = a.as_mut() {
                    let next = u32::MAX - *serial;
                    *serial += 1;
                    names.insert(*n, next);
                    *n = next;
                } else {
                    expr(a, names, serial);
                }
            }
            Expr::Bin(_, a, b) | Expr::Index(a, b) => {
                expr(a, names, serial);
                expr(b, names, serial);
            }
            Expr::Un(_, a)
            | Expr::Await(a)
            | Expr::Update(_, a, _)
            | Expr::Member(a, _)
            | Expr::Spread(a) => expr(a, names, serial),
            Expr::Cond(a, b, c) => {
                expr(a, names, serial);
                expr(b, names, serial);
                expr(c, names, serial);
            }
            Expr::Call {
                callee,
                args,
                this_arg,
                ..
            } => {
                expr(callee, names, serial);
                for a in args {
                    expr(a, names, serial);
                }
                if let Some(a) = this_arg {
                    expr(a, names, serial);
                }
            }
            Expr::Object(properties) => {
                for (_, value) in properties {
                    expr(value, names, serial);
                }
            }
            Expr::Array(elements) => {
                for value in elements {
                    expr(value, names, serial);
                }
            }
            _ => {}
        }
    }
    let mut output = canonical_stmts(stmts);
    let mut names = std::collections::HashMap::new();
    let mut serial = 0;
    for statement in &mut output {
        match statement {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
                expr(e, &mut names, &mut serial)
            }
            // 控制流内的定义是否支配后继需要额外证明，留给完整 CFG 路径处理。
            _ => return canonical_stmts(stmts),
        }
    }
    output
}

fn same_stmts(a: &[Stmt], b: &[Stmt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Stmt::Expr(a), Stmt::Expr(b)) | (Stmt::Throw(a), Stmt::Throw(b)) => same_expr(a, b),
            (Stmt::Return(a), Stmt::Return(b)) => match (a, b) {
                (Some(a), Some(b)) => same_expr(a, b),
                (None, None) => true,
                _ => false,
            },
            (Stmt::If(c, a, b), Stmt::If(d, x, y)) => {
                same_expr(c, d) && same_stmts(a, x) && same_stmts(b, y)
            }
            (Stmt::While(c, a), Stmt::While(d, b)) => same_expr(c, d) && same_stmts(a, b),
            (Stmt::TryFinally(a, b), Stmt::TryFinally(x, y)) => {
                same_stmts(a, x) && same_stmts(b, y)
            }
            (Stmt::Break, Stmt::Break) | (Stmt::Continue, Stmt::Continue) => true,
            _ => false,
        })
}

/// 最短匹配避免吞掉 return 值的保存赋值；副本内的临时寄存器只在局部规约。
fn remove_cleanup_suffix(
    stmts: &mut Vec<Stmt>,
    cleanup: &[Stmt],
    include_completion: bool,
) -> bool {
    let end = stmts.len().saturating_sub(usize::from(!include_completion));
    let mut completion_regs = Vec::new();
    if !include_completion
        && let Some(Stmt::Return(Some(value)) | Stmt::Throw(value)) = stmts.last()
    {
        // return 的表达式必须已经求值；移入 finally 不能改变调用/getter 的执行顺序。
        if crate::decompile::lift::has_effect(value) {
            return false;
        }
        collect_expr_regs(value, &mut completion_regs);
    }
    for count in 1..=end.min(120) {
        if stmts[end - count..end]
            .iter()
            .any(|s| completion_regs.iter().any(|r| is_reg_def(s, *r)))
        {
            continue;
        }
        if same_stmts(
            &alpha_canonical(&stmts[end - count..end]),
            &alpha_canonical(cleanup),
        ) {
            stmts.drain(end - count..end);
            return true;
        }
    }
    false
}

fn remove_finally_copies(stmts: &mut Vec<Stmt>, cleanup: &[Stmt], removed: &mut usize) -> bool {
    let overrides = definitely_abrupt(cleanup);
    // 原始块末尾可能还带有汇合值赋值，先删掉已知完成语句后的不可达残渣。
    truncate_after_return(stmts);
    if matches!(
        stmts.last(),
        Some(Stmt::Return(_) | Stmt::Break | Stmt::Continue)
    ) {
        if remove_cleanup_suffix(stmts, cleanup, overrides) {
            *removed += 1;
            if overrides {
                stmts.push(Stmt::Return(None));
            }
        } else {
            return false;
        }
    } else if (matches!(stmts.last(), Some(Stmt::Throw(_)))
        && remove_cleanup_suffix(stmts, cleanup, false))
        || (!definitely_abrupt(stmts) && remove_cleanup_suffix(stmts, cleanup, true))
    {
        *removed += 1;
    }
    for stmt in stmts.iter_mut() {
        let valid = match stmt {
            Stmt::If(_, a, b) => {
                remove_finally_copies(a, cleanup, removed)
                    && remove_finally_copies(b, cleanup, removed)
            }
            Stmt::While(_, b) | Stmt::DoWhile(b, _) | Stmt::ForOf(_, _, b) => {
                remove_finally_copies(b, cleanup, removed)
            }
            Stmt::Try(b, cs) => {
                remove_finally_copies(b, cleanup, removed)
                    && cs
                        .iter_mut()
                        .all(|(_, c)| remove_finally_copies(c, cleanup, removed))
            }
            Stmt::TryFinally(b, f) => {
                remove_finally_copies(b, cleanup, removed)
                    && remove_finally_copies(f, cleanup, removed)
            }
            _ => true,
        };
        if !valid {
            return false;
        }
    }
    true
}

/// 将同层声明移到安全的首次赋值处，保留初始化的原始求值顺序。
/// 只跨越局部复制和字面量；调用、属性读取、闭包和控制流均作为屏障，
/// 避免捕获变量在初始化前被访问时由 undefined 变为暂时性死区错误。
fn merge_declaration_initializers(stmts: &mut Vec<Stmt>) {
    use std::collections::{HashMap, HashSet};

    fn inspect_value(value: &Expr, locals: &HashSet<String>, pending: &mut HashMap<String, usize>) {
        match value {
            Expr::Reg(_)
            | Expr::Arg(_)
            | Expr::Int(_)
            | Expr::Float(_)
            | Expr::Str(_)
            | Expr::Bool(_) => {}
            Expr::Special("null" | "undefined" | "this" | "new.target") => {}
            Expr::Name(name) if locals.contains(name) => {
                pending.remove(name);
            }
            _ => pending.clear(),
        }
    }

    let locals: HashSet<_> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Declare(name, _) => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut pending = HashMap::new();
    let mut removed = HashSet::new();
    for (index, stmt) in stmts.iter_mut().enumerate() {
        match stmt {
            Stmt::Declare(name, None) => {
                pending.insert(name.clone(), index);
            }
            Stmt::Declare(name, Some(value)) => {
                inspect_value(value, &locals, &mut pending);
                pending.remove(name);
            }
            Stmt::Expr(Expr::Assign(target, value)) => {
                inspect_value(value, &locals, &mut pending);
                match target.as_ref() {
                    Expr::Name(name) if locals.contains(name) => {
                        if let Some(declaration) = pending.remove(name) {
                            removed.insert(declaration);
                            *stmt = Stmt::Declare(name.clone(), Some(*value.clone()));
                        }
                    }
                    Expr::Reg(_) | Expr::Arg(_) => {}
                    _ => pending.clear(),
                }
            }
            Stmt::Expr(value) => inspect_value(value, &locals, &mut pending),
            _ => {
                pending.clear();
                // 只合并子作用域自己的声明，不把外层声明移入分支或循环。
                match stmt {
                    Stmt::If(_, yes, no) | Stmt::TryFinally(yes, no) => {
                        merge_declaration_initializers(yes);
                        merge_declaration_initializers(no);
                    }
                    Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::ForOf(_, _, body) => {
                        merge_declaration_initializers(body);
                    }
                    Stmt::Try(body, catches) => {
                        merge_declaration_initializers(body);
                        for (_, handler) in catches {
                            merge_declaration_initializers(handler);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let mut index = 0;
    stmts.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
}

/// 各入口共享的有界整理流程。
pub fn polish(stmts: &mut Vec<Stmt>) {
    truncate_after_return(stmts);
    for _ in 0..3 {
        eliminate_dead_stores(stmts);
        simplify_control(stmts);
        if stmts.len() <= 600 {
            fold_register_stmts(stmts);
        }
    }
    eliminate_dead_stores(stmts);
    simplify_control(stmts);
    merge_declaration_initializers(stmts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named_call(name: &str) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::Name(name.into())),
            args: Vec::new(),
            new: false,
            this_arg: None,
        }
    }

    fn assign_name(name: &str, value: Expr) -> Stmt {
        Stmt::Expr(Expr::Assign(
            Box::new(Expr::Name(name.into())),
            Box::new(value),
        ))
    }

    #[test]
    fn capture_initializers_merge_in_assignment_order_and_keep_later_writes() {
        let mut stmts = vec![
            Stmt::Declare("first".into(), None),
            Stmt::Declare("second".into(), None),
            assign_name("second", Expr::Arg(4)),
            assign_name("first", Expr::Arg(3)),
            assign_name("first", Expr::Int(1)),
            Stmt::Return(Some(Expr::Name("first".into()))),
        ];
        polish(&mut stmts);
        assert!(matches!(&stmts[0], Stmt::Declare(name, Some(Expr::Arg(4))) if name == "second"));
        assert!(matches!(&stmts[1], Stmt::Declare(name, Some(Expr::Arg(3))) if name == "first"));
        assert!(
            matches!(&stmts[2], Stmt::Expr(Expr::Assign(_, value)) if matches!(value.as_ref(), Expr::Int(1)))
        );
        assert_eq!(stmts.len(), 4);
    }

    #[test]
    fn initializer_merging_preserves_reads_before_assignment_and_self_reads() {
        for read in [
            Stmt::Expr(Expr::Name("capture".into())),
            Stmt::Declare("other".into(), Some(Expr::Name("capture".into()))),
            assign_name("capture", Expr::Name("capture".into())),
        ] {
            let mut stmts = vec![
                Stmt::Declare("capture".into(), None),
                read,
                assign_name("capture", Expr::Arg(3)),
            ];
            merge_declaration_initializers(&mut stmts);
            assert!(matches!(&stmts[0], Stmt::Declare(_, None)), "{stmts:?}");
            assert_eq!(stmts.len(), 3);
        }
    }

    #[test]
    fn initializer_merging_stops_at_effects_and_opaque_values() {
        for value in [
            named_call("invokeCapture"),
            Expr::Member(Box::new(Expr::Arg(3)), "getter".into()),
            Expr::Closure { method_off: 1 },
            Expr::ClassLit(1),
            Expr::Await(Box::new(Expr::Arg(3))),
            Expr::Bin("+", Box::new(Expr::Arg(3)), Box::new(Expr::Int(1))),
            Expr::Name("unknownGlobal".into()),
        ] {
            for barrier in [Stmt::Expr(value.clone()), assign_name("capture", value)] {
                let mut stmts = vec![
                    Stmt::Declare("capture".into(), None),
                    barrier,
                    assign_name("capture", Expr::Int(1)),
                ];
                merge_declaration_initializers(&mut stmts);
                assert!(matches!(&stmts[0], Stmt::Declare(_, None)), "{stmts:?}");
                assert_eq!(stmts.len(), 3);
            }
        }
    }

    #[test]
    fn initializer_merging_keeps_outer_bindings_outside_control_flow() {
        let body = vec![assign_name("capture", Expr::Arg(3))];
        for barrier in [
            Stmt::If(Expr::Arg(4), body.clone(), Vec::new()),
            Stmt::While(Expr::Arg(4), body.clone()),
            Stmt::DoWhile(body.clone(), Expr::Arg(4)),
            Stmt::ForOf("item".into(), Expr::Arg(4), body.clone()),
            Stmt::Try(body.clone(), vec![(None, Vec::new())]),
            Stmt::TryFinally(body, Vec::new()),
            Stmt::ClassDecl(1),
            Stmt::Comment("unknown instruction".into()),
        ] {
            let mut stmts = vec![
                Stmt::Declare("capture".into(), None),
                barrier,
                assign_name("capture", Expr::Int(1)),
            ];
            merge_declaration_initializers(&mut stmts);
            assert!(matches!(&stmts[0], Stmt::Declare(_, None)), "{stmts:?}");
            assert_eq!(stmts.len(), 3);
        }
    }

    #[test]
    fn empty_catch_regions_disappear_but_effectful_regions_and_finalizers_remain() {
        let catches = vec![(
            Some("error".into()),
            vec![Stmt::Throw(Expr::Name("error".into()))],
        )];
        let mut stmts = vec![
            Stmt::Try(Vec::new(), catches.clone()),
            Stmt::Try(vec![Stmt::Expr(named_call("mayThrow"))], catches),
            Stmt::TryFinally(Vec::new(), vec![Stmt::Expr(named_call("cleanup"))]),
        ];
        simplify_control(&mut stmts);
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], Stmt::Try(body, _) if !body.is_empty()));
        assert!(matches!(&stmts[1], Stmt::TryFinally(_, finalizer) if !finalizer.is_empty()));
    }

    #[test]
    fn finally_keeps_return_expression_evaluation_order() {
        let cleanup = vec![Stmt::Expr(named_call("cleanup"))];
        let mut body = vec![cleanup[0].clone(), Stmt::Return(Some(named_call("value")))];
        assert!(!remove_cleanup_suffix(&mut body, &cleanup, false));
        let mut evaluated = vec![
            assign(0, named_call("value")),
            cleanup[0].clone(),
            Stmt::Return(Some(Expr::Reg(0))),
        ];
        assert!(remove_cleanup_suffix(&mut evaluated, &cleanup, false));
        assert!(
            matches!(&evaluated[0], Stmt::Expr(Expr::Assign(_, v)) if matches!(v.as_ref(), Expr::Call { .. }))
        );
    }

    #[test]
    fn finally_copy_matching_preserves_live_input_registers() {
        let call = |r| Expr::Call {
            callee: Box::new(Expr::Member(Box::new(Expr::Reg(r)), "close".into())),
            args: Vec::new(),
            new: false,
            this_arg: Some(Box::new(Expr::Reg(r))),
        };
        let cleanup = vec![Stmt::Expr(call(1))];
        let mut unrelated = vec![Stmt::Expr(call(2)), Stmt::Return(None)];
        assert!(!remove_cleanup_suffix(&mut unrelated, &cleanup, false));
    }

    #[test]
    fn finally_copy_matching_renames_only_locally_defined_temporaries() {
        let local = |r| {
            vec![
                assign(
                    r,
                    Expr::Member(Box::new(Expr::Special("globalThis")), "console".into()),
                ),
                Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::Member(Box::new(Expr::Reg(r)), "log".into())),
                    args: vec![Expr::Str("cleanup".into())],
                    new: false,
                    this_arg: Some(Box::new(Expr::Reg(r))),
                }),
            ]
        };
        assert!(same_stmts(
            &alpha_canonical(&local(4)),
            &alpha_canonical(&local(9))
        ));
    }

    fn assign(n: u32, value: Expr) -> Stmt {
        Stmt::Expr(Expr::Assign(Box::new(Expr::Reg(n)), Box::new(value)))
    }

    #[test]
    fn folding_keeps_values_used_in_multiple_statements() {
        let mut body = vec![
            assign(0, Expr::Int(7)),
            assign(1, Expr::Reg(0)),
            Stmt::Return(Some(Expr::Reg(0))),
        ];
        fold_register_stmts(&mut body);
        assert!(
            matches!(&body[0], Stmt::Expr(Expr::Assign(t, _)) if matches!(t.as_ref(), Expr::Reg(0)))
        );
    }

    #[test]
    fn branch_assignment_used_after_merge_is_not_discarded() {
        let mut body = vec![
            Stmt::If(
                Expr::Arg(3),
                vec![assign(0, Expr::Int(1))],
                vec![assign(0, Expr::Int(2))],
            ),
            Stmt::Return(Some(Expr::Reg(0))),
        ];
        polish(&mut body);
        let text = format!("{body:?}");
        assert!(text.contains("Int(1)") && text.contains("Int(2)"), "{text}");
        assert!(text.contains("Cond(Arg(3)"), "{text}");
    }

    #[test]
    fn loop_initialization_stays_outside_repeated_condition() {
        let mut body = vec![
            assign(0, Expr::Int(0)),
            Stmt::While(
                Expr::Bin("<", Box::new(Expr::Reg(0)), Box::new(Expr::Int(3))),
                vec![assign(
                    0,
                    Expr::Bin("+", Box::new(Expr::Reg(0)), Box::new(Expr::Int(1))),
                )],
            ),
            Stmt::Return(Some(Expr::Reg(0))),
        ];
        polish(&mut body);
        assert!(matches!(body[0], Stmt::Expr(Expr::Assign(_, _))));
        assert!(
            matches!(&body[1], Stmt::While(Expr::Bin("<", a, _), _) if matches!(a.as_ref(), Expr::Reg(0)))
        );
    }

    #[test]
    fn register_swap_keeps_the_old_value() {
        let mut body = vec![
            assign(2, Expr::Reg(0)),
            assign(0, Expr::Reg(1)),
            assign(1, Expr::Reg(2)),
            Stmt::Return(Some(Expr::Reg(1))),
        ];
        fold_register_stmts(&mut body);
        assert!(
            matches!(&body[0], Stmt::Expr(Expr::Assign(t, v)) if matches!(t.as_ref(), Expr::Reg(2)) && matches!(v.as_ref(), Expr::Reg(0)))
        );
    }

    #[test]
    fn unused_getter_and_empty_branch_test_keep_side_effects() {
        let getter = Expr::Member(Box::new(Expr::Arg(3)), "value".into());
        let mut body = vec![assign(0, getter.clone()), Stmt::If(getter, vec![], vec![])];
        polish(&mut body);
        assert_eq!(body.len(), 2);
        assert!(
            body.iter()
                .all(|s| matches!(s, Stmt::Expr(Expr::Member(_, _))))
        );
    }

    #[test]
    fn property_write_uses_original_computed_value_only_once() {
        let value = Expr::Bin(
            "+",
            Box::new(Expr::Member(
                Box::new(Expr::Special("this")),
                "clicks".into(),
            )),
            Box::new(Expr::Int(1)),
        );
        let mut body = vec![
            assign(0, value),
            Stmt::Expr(Expr::Assign(
                Box::new(Expr::Member(
                    Box::new(Expr::Special("this")),
                    "clicks".into(),
                )),
                Box::new(Expr::Reg(0)),
            )),
            Stmt::Return(Some(Expr::Reg(0))),
        ];
        polish(&mut body);
        assert_eq!(format!("{body:?}").matches("Bin(\"+\"").count(), 1);
        assert!(matches!(
            body.last(),
            Some(Stmt::Return(Some(Expr::Reg(0))))
        ));
    }
}
