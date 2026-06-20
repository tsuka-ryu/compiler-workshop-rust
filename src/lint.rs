//! 簡易 linter (節11)。
//!
//! 節 5 の [`Visit`](crate::visit::Visit) trait を土台に、`ast_span` AST を走査して
//! 警告を集める。oxc の linter と同じく **ルール毎に visitor を 1 個** 作り、
//! [`lint`] が全ルールを走らせて結果を結合する。
//!
//! ルール:
//! - `constant-condition` — 三項演算子の test が定数 (`true ? a : b`)
//! - `unreachable-code` — `return` より後の文 (未実装)
//! - `no-unused-vars` — 宣言されたが参照されない変数 (未実装)

use crate::ast_span::{Expression, Parameter, Statement};
use crate::tokenize_span::Span;
use crate::visit::{walk_expression, walk_statement, Visit};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct LintWarning {
    pub rule_name: String,
    pub message: String,
    pub span: Span,
}

/// 全ルールを走らせて警告を集める。
pub fn lint(statements: &[Statement]) -> Vec<LintWarning> {
    let mut warnings = Vec::new();
    warnings.extend(constant_condition(statements));
    warnings.extend(unreachable_code(statements));
    warnings.extend(no_unused_vars(statements));
    warnings
}

// --- rule: constant-condition ---

/// 三項演算子 `test ? a : b` の `test` が真偽値リテラルなら、常に一方に倒れるので警告。
fn constant_condition(statements: &[Statement]) -> Vec<LintWarning> {
    let mut rule = ConstantCondition {
        warnings: Vec::new(),
    };
    for stmt in statements {
        rule.visit_statement(stmt);
    }
    rule.warnings
}

struct ConstantCondition {
    warnings: Vec<LintWarning>,
}

impl Visit for ConstantCondition {
    fn visit_expression(&mut self, expr: &Expression) {
        if let Expression::Conditional { test, span, .. } = expr {
            if let Expression::Boolean { value, .. } = test.as_ref() {
                let branch = if *value { "consequent" } else { "alternate" };
                self.warnings.push(LintWarning {
                    rule_name: "constant-condition".to_string(),
                    message: format!("Conditional test is always {value}; only the {branch} runs"),
                    span: *span,
                });
            }
        }
        // 子も辿る (ネストした三項も拾う)
        walk_expression(self, expr);
    }
}

// --- rule: unreachable-code ---

/// `return` より後ろの文は実行されないので警告。文の列 (トップレベルとアロー関数の body) を
/// 走り、`Return` が出たら以降の文を全部報告する。
fn unreachable_code(statements: &[Statement]) -> Vec<LintWarning> {
    let mut rule = UnreachableCode {
        warnings: Vec::new(),
    };
    rule.check_block(statements);
    rule.warnings
}

struct UnreachableCode {
    warnings: Vec<LintWarning>,
}

impl UnreachableCode {
    /// 1 つの文の列を見て、最初の `Return` 以降を unreachable として報告する。
    /// 各文の中のアロー関数 body も再帰的に検査する。
    fn check_block(&mut self, statements: &[Statement]) {
        let mut returned = false;
        for stmt in statements {
            if returned {
                self.warnings.push(LintWarning {
                    rule_name: "unreachable-code".to_string(),
                    message: "Unreachable code after return".to_string(),
                    span: stmt.span(),
                });
            }
            // ネストしたアロー関数の body も検査 (この文自体が到達不能でも中は見る)。
            self.visit_statement(stmt);
            if let Statement::Return { .. } = stmt {
                returned = true;
            }
        }
    }
}

impl Visit for UnreachableCode {
    fn visit_expression(&mut self, expr: &Expression) {
        // アロー関数に入ったら、その body を 1 つの新しいブロックとして検査する。
        if let Expression::ArrowFunction { body, .. } = expr {
            self.check_block(body);
            // body は check_block 内で辿るので、ここでは params / return_type だけ辿れば十分だが
            // 簡単のため子の walk はスキップ (body の二重訪問を避ける)。
            return;
        }
        walk_expression(self, expr);
    }
}

// --- rule: no-unused-vars ---

/// 宣言されたが一度も参照されない変数を警告する。節 10 の symbol table の発想を、
/// span を載せた最小版で lint 側に持つ (`naming_indexed` は `crate::parse` AST 上で span を
/// 持たないため、ここでは `ast_span` 上に作り直す)。
fn no_unused_vars(statements: &[Statement]) -> Vec<LintWarning> {
    let mut rule = NoUnusedVars {
        scopes: vec![HashMap::new()],
        warnings: Vec::new(),
    };
    rule.check_block(statements);
    // トップレベルスコープを閉じて、未使用のトップレベル宣言を回収する。
    rule.leave_scope();
    rule.warnings
}

/// 1 つの宣言の状態。`used` は参照されたら立つ。
struct Binding {
    span: Span,
    used: bool,
}

struct NoUnusedVars {
    /// スコープのスタック。各スコープは 名前 → Binding。
    scopes: Vec<HashMap<String, Binding>>,
    warnings: Vec<LintWarning>,
}

impl NoUnusedVars {
    fn declare(&mut self, name: &str, span: Span) {
        self.scopes
            .last_mut()
            .expect("scopes is non-empty")
            .insert(name.to_string(), Binding { span, used: false });
    }

    /// 名前を参照済みにする。内側のスコープから外側へ探し、最初に見つかった方を立てる。
    fn mark_used(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.get_mut(name) {
                binding.used = true;
                return;
            }
        }
    }

    /// スコープを抜けるとき、未使用の binding を警告に積む。
    fn leave_scope(&mut self) {
        let scope = self.scopes.pop().expect("scopes is non-empty");
        for (name, binding) in scope {
            if !binding.used {
                self.warnings.push(LintWarning {
                    rule_name: "no-unused-vars".to_string(),
                    message: format!("'{name}' is declared but never used"),
                    span: binding.span,
                });
            }
        }
    }

    /// 文の列を 1 スコープとして検査する。`naming_indexed` と同じく init を先に見てから declare。
    fn check_block(&mut self, statements: &[Statement]) {
        for stmt in statements {
            match stmt {
                Statement::ConstDeclaration {
                    name, init, span, ..
                } => {
                    self.visit_expression(init);
                    self.declare(name, *span);
                }
                Statement::Return { argument, .. } => {
                    if let Some(arg) = argument {
                        self.visit_expression(arg);
                    }
                }
            }
        }
    }
}

impl Visit for NoUnusedVars {
    fn visit_expression(&mut self, expr: &Expression) {
        match expr {
            Expression::Identifier { name, .. } => self.mark_used(name),
            Expression::ArrowFunction { params, body, .. } => {
                // 新スコープ: param を宣言し body を検査、抜けるとき未使用 param も拾う。
                self.scopes.push(HashMap::new());
                for param in params {
                    let Parameter { name, span, .. } = param;
                    self.declare(name, *span);
                }
                self.check_block(body);
                self.leave_scope();
            }
            _ => walk_expression(self, expr),
        }
    }

    fn visit_statement(&mut self, stmt: &Statement) {
        walk_statement(self, stmt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_result::parse_result;
    use crate::tokenize_span::tokenize_span;

    fn parse(src: &str) -> Vec<Statement> {
        parse_result(tokenize_span(src)).unwrap()
    }

    // --- constant-condition ---

    #[test]
    fn flags_constant_true_ternary() {
        let warnings = constant_condition(&parse("const x = true ? 1 : 2;"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].rule_name, "constant-condition");
        assert!(warnings[0].message.contains("consequent"));
    }

    #[test]
    fn flags_constant_false_ternary() {
        let warnings = constant_condition(&parse("const x = false ? 1 : 2;"));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("alternate"));
    }

    #[test]
    fn no_warning_for_dynamic_ternary() {
        assert_eq!(constant_condition(&parse("const x = a ? 1 : 2;")), vec![]);
    }

    // --- unreachable-code ---

    #[test]
    fn flags_code_after_return() {
        let warnings = unreachable_code(&parse(
            "const f = (x) => { return x; const y = 1; };",
        ));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].rule_name, "unreachable-code");
    }

    #[test]
    fn no_warning_when_return_is_last() {
        let warnings = unreachable_code(&parse("const f = (x) => { return x; };"));
        assert_eq!(warnings, vec![]);
    }

    #[test]
    fn flags_multiple_statements_after_return() {
        let warnings = unreachable_code(&parse(
            "const f = (x) => { return x; const a = 1; const b = 2; };",
        ));
        assert_eq!(warnings.len(), 2);
    }

    // --- no-unused-vars ---

    #[test]
    fn flags_unused_const() {
        let warnings = no_unused_vars(&parse("const x = 1;"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].rule_name, "no-unused-vars");
        assert!(warnings[0].message.contains("x"));
    }

    #[test]
    fn no_warning_when_used() {
        // y は x を参照、x は使用済み。y はトップレベルで未使用なので 1 件。
        let warnings = no_unused_vars(&parse("const x = 1; const y = x;"));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("y"));
    }

    #[test]
    fn unused_arrow_param() {
        let warnings = no_unused_vars(&parse("const f = (x) => { return 1; };"));
        // x (param) は未使用。f は未使用。
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|w| w.message.contains("x")));
        assert!(warnings.iter().any(|w| w.message.contains("f")));
    }

    #[test]
    fn used_arrow_param() {
        let warnings = no_unused_vars(&parse("const f = (x) => { return x; };"));
        // x は使用済み。f だけ未使用。
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("f"));
    }

    // --- lint() 統合 ---

    #[test]
    fn lint_runs_all_rules() {
        // x は未使用 + 定数三項。warnings は両ルールから出る。
        let warnings = lint(&parse("const x = true ? 1 : 2;"));
        let rules: Vec<&str> = warnings.iter().map(|w| w.rule_name.as_str()).collect();
        assert!(rules.contains(&"constant-condition"));
        assert!(rules.contains(&"no-unused-vars"));
    }
}