//! 被丢弃的转换运算保留副作用与异常，严格相等和布尔运算仍可消除。
//! @author ljlVink

use decompile::lift::{Expr, Stmt, has_effect};
use pedal_abc_bridge::{decompile, file::AbcFile};

use super::common;

#[test]
fn coercing_operations_are_effectful_even_for_register_operands() {
    for operator in [
        "+",
        "-",
        "*",
        "/",
        "%",
        "**",
        "<",
        ">",
        "<=",
        ">=",
        "==",
        "!=",
        "&",
        "|",
        "^",
        "<<",
        ">>",
        ">>>",
        "in",
        "instanceof",
    ] {
        let expr = Expr::Bin(operator, Box::new(Expr::Arg(3)), Box::new(Expr::Int(1)));
        assert!(
            has_effect(&expr),
            "discarded {operator} must preserve coercion/throw"
        );
        let mut body = vec![Stmt::Expr(Expr::Assign(
            Box::new(Expr::Reg(0)),
            Box::new(expr),
        ))];
        decompile::structure::polish(&mut body);
        assert!(
            matches!(body.as_slice(), [Stmt::Expr(Expr::Bin(op, _, _))] if *op == operator),
            "{body:?}"
        );
    }
    for operator in ["-", "+", "~", "Number"] {
        assert!(has_effect(&Expr::Un(operator, Box::new(Expr::Arg(3)))));
    }
}

#[test]
fn strict_equality_and_boolean_checks_remain_pure() {
    let mut body = vec![];
    for operator in ["===", "!==", "&&", "||", "??"] {
        let expr = Expr::Bin(operator, Box::new(Expr::Arg(3)), Box::new(Expr::Int(1)));
        assert!(!has_effect(&expr), "{operator}");
        body.push(Stmt::Expr(expr));
    }
    for operator in ["!", "typeof", "Boolean", "void"] {
        let expr = Expr::Un(operator, Box::new(Expr::Arg(3)));
        assert!(!has_effect(&expr), "{operator}");
        body.push(Stmt::Expr(expr));
    }
    decompile::structure::polish(&mut body);
    assert!(
        body.is_empty(),
        "pure discarded checks should still be removed: {body:?}"
    );
}

#[test]
fn compiler_fixtures_keep_discarded_coercion_inside_protected_loop() {
    for (mode, path) in [
        ("debug", "coercion_recovery/debug.abc"),
        ("release", "coercion_recovery/release.abc"),
    ] {
        let bytes = common::read_fixture(path);
        let file = AbcFile::parse(bytes).unwrap();
        let record = file
            .classes
            .iter()
            .find(|r| r.name.contains("CoercionRecovery"))
            .unwrap();
        let source = decompile::decompile_record(&file, record).unwrap().source;
        assert!(
            !source.contains("Conservative fallback"),
            "{mode}: {source}"
        );
        assert_eq!(
            source.matches(" + 1;").count(),
            2,
            "ordinary and protected discarded add: {mode}\n{source}"
        );
        assert!(
            source.contains(" == 1;"),
            "discarded equality: {mode}\n{source}"
        );
        assert!(source.contains("try {"), "{mode}\n{source}");
    }
}
