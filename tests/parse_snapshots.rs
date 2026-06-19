use compiler_workshop::parse_resilient::parse_resilient;
use compiler_workshop::parse_result::parse_result;
use compiler_workshop::tokenize_span::tokenize_span;

#[test]
fn t01_const_decl_with_binary() {
    let stmts = parse_result(tokenize_span("const x = 1 + 2;")).unwrap();
    insta::assert_debug_snapshot!(stmts);
}

#[test]
fn t02_precedence_mul_then_add() {
    // 1 + 2 * 3 → Add(1, Mul(2, 3)) のはず
    let stmts = parse_result(tokenize_span("const x = 1 + 2 * 3;")).unwrap();
    insta::assert_debug_snapshot!(stmts);
}

#[test]
fn t03_ternary() {
    let stmts = parse_result(tokenize_span("const x = true ? 1 : 2;")).unwrap();
    insta::assert_debug_snapshot!(stmts);
}

#[test]
fn t04_array_literal() {
    let stmts = parse_result(tokenize_span("const xs = [1, 2, 3];")).unwrap();
    insta::assert_debug_snapshot!(stmts);
}

#[test]
fn t05_arrow_function() {
    let stmts = parse_result(tokenize_span(
        "const f = (x: number): number => { return x + 1; };",
    ))
    .unwrap();
    insta::assert_debug_snapshot!(stmts);
}

#[test]
fn t06_call_and_member() {
    let stmts = parse_result(tokenize_span("const y = f(a, b)[0];")).unwrap();
    insta::assert_debug_snapshot!(stmts);
}

#[test]
fn t07_resilient_well_formed() {
    // 正常入力: errors 空、panicked=false、parse_span と同じ木
    let ret = parse_resilient(tokenize_span("const x = 1 + 2;"));
    insta::assert_debug_snapshot!(ret);
}

#[test]
fn t08_resilient_recoverable_missing_semicolon() {
    // ; 欠落: 木は残り、errors に1件(recoverable tier)
    let ret = parse_resilient(tokenize_span("const x = 5"));
    insta::assert_debug_snapshot!(ret);
}

#[test]
fn t09_resilient_fatal_discards_tree() {
    // 式が壊れている: fatal。statements 空 + panicked=true + fatal 1件
    let ret = parse_resilient(tokenize_span("const x = ;"));
    insta::assert_debug_snapshot!(ret);
}
