use crate::parse::{BinaryOp, Expression, Statement};
use std::collections::HashMap;

pub type TypeId = usize;

#[derive(Debug, Clone)]
enum DbEntry {
    /// 未割り当ての型変数
    Var,
    /// 具象型（"Number", "String", "Boolean", "Void", "Array"）
    Concrete(String),
    /// 別の TypeId への symlink（unionfind 的）
    Symlink(TypeId),
    /// 関数型 (params) -> return_type
    Function {
        params: Vec<TypeId>,
        return_type: TypeId,
    },
}

#[derive(Debug, Clone)]
struct TypeScheme {
    /// 量化された型変数（forall に相当）
    vars: Vec<TypeId>,
    /// 型本体
    body: TypeId,
}

impl TypeScheme {
    /// 多相でない（普通の）型をスキームとして包む
    fn monomorphic(body: TypeId) -> Self {
        Self {
            vars: Vec::new(),
            body,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct TypeError {
    pub message: String,
}

struct TypeChecker {
    db: Vec<DbEntry>,
    scope: HashMap<String, TypeScheme>, // ← TypeId から TypeScheme へ
    errors: Vec<TypeError>,
}

impl TypeChecker {
    fn new() -> Self {
        Self {
            db: Vec::new(),
            scope: HashMap::new(),
            errors: Vec::new(),
        }
    }

    fn fresh_var(&mut self) -> TypeId {
        let id = self.db.len();
        self.db.push(DbEntry::Var);
        id
    }

    fn concrete(&mut self, name: &str) -> TypeId {
        let id = self.db.len();
        self.db.push(DbEntry::Concrete(name.to_string()));
        id
    }

    // Symlinkを辿って最終的なTypeIdを返す（path compression）
    fn resolve(&mut self, id: TypeId) -> TypeId {
        match self.db[id].clone() {
            DbEntry::Symlink(next) => {
                let ultimate = self.resolve(next);
                if ultimate != next {
                    self.db[id] = DbEntry::Symlink(ultimate);
                }
                ultimate
            }
            _ => id, // VarかConcreteはそのまま
        }
    }

    // resolveした先がConcreteなら名前を返す
    fn concrete_name(&mut self, id: TypeId) -> Option<String> {
        let resolved = self.resolve(id);
        match &self.db[resolved] {
            DbEntry::Concrete(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn report(&mut self, message: String) {
        self.errors.push(TypeError { message });
    }

    /// 2つの TypeId を「同じ型」として統合する
    fn unify(&mut self, a: TypeId, b: TypeId) -> bool {
        let a = self.resolve(a);
        let b = self.resolve(b);

        // 同じものにたどり着いた -> すでに統合済み
        if a == b {
            return true;
        }

        match (self.db[a].clone(), self.db[b].clone()) {
            // Var が絡む → Var を Symlink にする（先に処理）
            (DbEntry::Var, _) => {
                self.db[a] = DbEntry::Symlink(b);
                true
            }
            (_, DbEntry::Var) => {
                self.db[b] = DbEntry::Symlink(a);
                true
            }
            // Concrete 同士
            (DbEntry::Concrete(name_a), DbEntry::Concrete(name_b)) => {
                if name_a == name_b {
                    true
                } else {
                    self.report(format!(
                        "Type mismatch: cannot unify {name_a} with {name_b}"
                    ));
                    false
                }
            }
            // Function 同士 → arity 確認 + 再帰的 unify
            (
                DbEntry::Function {
                    params: pa,
                    return_type: ra,
                },
                DbEntry::Function {
                    params: pb,
                    return_type: rb,
                },
            ) => {
                if pa.len() != pb.len() {
                    self.report(format!(
                        "Function arity mismatch: expected {} args, got {}",
                        pa.len(),
                        pb.len()
                    ));
                    return false;
                }
                let mut ok = true;
                for (x, y) in pa.iter().zip(pb.iter()) {
                    if !self.unify(*x, *y) {
                        ok = false;
                    }
                }
                if !self.unify(ra, rb) {
                    ok = false;
                }
                ok
            }

            // Function と Concrete の組み合わせ → 不一致
            (DbEntry::Function { .. }, DbEntry::Concrete(name))
            | (DbEntry::Concrete(name), DbEntry::Function { .. }) => {
                self.report(format!("Type mismatch: cannot unify function with {name}"));
                false
            }

            _ => unreachable!("resolve should collapse Symlinks"),
        }
    }

    fn visit_statement(&mut self, stmt: &Statement) -> TypeId {
        match stmt {
            Statement::ConstDeclaration { name, init, .. } => {
                let init_type = self.visit_expression(init);
                let scheme = self.generalize(init_type);
                self.scope.insert(name.clone(), scheme);
                init_type
            }

            Statement::Return { argument } => match argument {
                Some(expr) => self.visit_expression(expr),
                None => self.concrete("Void"),
            },
        }
    }

    fn visit_expression(&mut self, expr: &Expression) -> TypeId {
        match expr {
            Expression::Number(_) => self.concrete("Number"),
            Expression::String(_) => self.concrete("String"),
            Expression::Boolean(_) => self.concrete("Boolean"),

            Expression::Identifier(name) => {
                // スコープにあればそれ、なければ fresh var（NamingErrorとは別として処理）
                if let Some(scheme) = self.scope.get(name).cloned() {
                    self.instantiate(&scheme)
                } else {
                    self.fresh_var()
                }
            }

            Expression::Binary { left, op, right } => {
                let left_type = self.visit_expression(left);
                let right_type = self.visit_expression(right);

                match op {
                    BinaryOp::Add => {
                        // 左右が同じ型であるべき
                        let left_concrete = self.concrete_name(left_type);
                        let right_concrete = self.concrete_name(right_type);

                        if let (Some(l), Some(r)) = (&left_concrete, &right_concrete) {
                            if l != r {
                                self.report(format!(
                                    "Type mismatch in binary operation: cannot add {l} to {r}"
                                ));
                                return self.concrete("Number");
                            }
                        }

                        self.unify(left_type, right_type);
                        left_type
                    }
                    BinaryOp::Multiply => {
                        // 左右が Number であるべき
                        let number = self.concrete("Number");

                        let left_concrete = self.concrete_name(left_type);
                        if let Some(name) = &left_concrete {
                            if name != "Number" {
                                self.report(format!(
                        "Type mismatch: expected Number for left operand of '*' operator, got {name}"
                    ));
                            }
                        } else {
                            self.unify(left_type, number);
                        }

                        let right_concrete = self.concrete_name(right_type);
                        if let Some(name) = &right_concrete {
                            if name != "Number" {
                                self.report(format!(
                        "Type mismatch: expected Number for right operand of '*' operator, got {name}"
                    ));
                            }
                        } else {
                            self.unify(right_type, number);
                        }

                        number
                    }
                }
            }
            Expression::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let test_type = self.visit_expression(test);
                let consequent_type = self.visit_expression(consequent);
                let alternate_type = self.visit_expression(alternate);

                // test は Boolean であるべき
                let boolean = self.concrete("Boolean");
                let test_concrete = self.concrete_name(test_type);
                if let Some(name) = &test_concrete {
                    if name != "Boolean" {
                        self.report(format!(
                            "Type mismatch in ternary: condition must be Boolean, got {name}"
                        ));
                    }
                } else {
                    self.unify(test_type, boolean);
                }

                // consequent と alternate は同じ型であるべき
                let consequent_concrete = self.concrete_name(consequent_type);
                let alternate_concrete = self.concrete_name(alternate_type);
                if let (Some(c), Some(a)) = (&consequent_concrete, &alternate_concrete) {
                    if c != a {
                        self.report(format!(
                "Type mismatch in ternary: branches must have the same type, got {c} and {a}"
            ));
                    }
                } else {
                    self.unify(consequent_type, alternate_type);
                }

                consequent_type
            }

            Expression::Array(elements) => {
                if elements.is_empty() {
                    return self.concrete("Array");
                }

                // 最初の要素を基準にする
                let first_type = self.visit_expression(&elements[0]);
                let first_concrete = self.concrete_name(first_type);

                for elem in &elements[1..] {
                    let elem_type = self.visit_expression(elem);
                    let elem_concrete = self.concrete_name(elem_type);

                    if let (Some(f), Some(e)) = (&first_concrete, &elem_concrete) {
                        if f != e {
                            self.report(format!(
                    "Type mismatch in array literal: array elements must have consistent types, found {f} and {e}"
                ));
                            continue;
                        }
                    }

                    self.unify(first_type, elem_type);
                }

                self.concrete("Array")
            }

            Expression::ArrowFunction { params, body, .. } => {
                // 関数を抜けたらスコープを元に戻すために保存
                let saved_scope = self.scope.clone();

                // 各 param に fresh varを割り当てて、スコープに登録
                let mut param_types = Vec::new();
                for param in params {
                    let param_type = self.fresh_var();
                    self.scope
                        .insert(param.name.clone(), TypeScheme::monomorphic(param_type));
                    param_types.push(param_type);
                }

                // body を訪問（最後の文の型を関数の戻り値型とする）
                let mut body_type = self.concrete("Void");
                for stmt in body {
                    body_type = self.visit_statement(stmt);
                }

                // スコープを復元（paramがスコープから消える）
                self.scope = saved_scope;

                // 関数型のエントリを作って返す
                let function_id = self.db.len();
                self.db.push(DbEntry::Function {
                    params: param_types,
                    return_type: body_type,
                });
                function_id
            }

            Expression::Call { callee, arguments } => {
                // callee の型を取得
                let callee_type = self.visit_expression(callee);

                // 各引数の型を取得
                let arg_types: Vec<TypeId> = arguments
                    .iter()
                    .map(|arg| self.visit_expression(arg))
                    .collect();

                // 戻り値の型（未知なので fresh var）
                let return_type = self.fresh_var();

                // 「期待される関数型」を作る: (arg_types) → return_type
                let expected_id = self.db.len();
                self.db.push(DbEntry::Function {
                    params: arg_types,
                    return_type,
                });

                // callee の実際の型と期待型を unify
                self.unify(callee_type, expected_id);

                return_type
            }

            Expression::Member { object, index } => {
                self.visit_expression(object);
                self.visit_expression(index);
                self.fresh_var() // 要素型は不明
            }
        }
    }

    /// スキームを TypeId に展開する
    fn instantiate(&mut self, scheme: &TypeScheme) -> TypeId {
        if scheme.vars.is_empty() {
            return scheme.body;
        }

        // 量化された各変数に新しい fresh var を割り当てる
        let mut subst: HashMap<TypeId, TypeId> = HashMap::new();
        for &v in &scheme.vars {
            let fresh = self.fresh_var();
            subst.insert(v, fresh);
        }

        // 置換しながら型のコピーを作る
        self.copy_with_subst(scheme.body, &subst)
    }

    /// 型に含まれる Var の TypeId をすべて集める
    /// 結果は out に追加（呼び出し側が用意した HashSet を渡す）
    /// 自由変数（free variable) -> 「自由」とは「まだ束縛されていない」の意
    /// T → T   ← この T は「自由」（量化されてない）
    /// ∀T. T → T   ← この T は「束縛」されてる（∀ で量化されてる）
    fn free_vars(&mut self, id: TypeId, out: &mut std::collections::HashSet<TypeId>) {
        let id = self.resolve(id);
        match self.db[id].clone() {
            DbEntry::Var => {
                out.insert(id);
            }
            DbEntry::Concrete(_) => {}
            DbEntry::Function {
                params,
                return_type,
            } => {
                for p in &params {
                    self.free_vars(*p, out);
                }
                self.free_vars(return_type, out);
            }
            DbEntry::Symlink(_) => unreachable!("resolve should collapse Symlinks"),
        }
    }

    /// 現在のスコープ全体に含まれる自由型変数を集める
    fn env_free_vars(&mut self) -> std::collections::HashSet<TypeId> {
        let mut out = std::collections::HashSet::new();
        // scope を clone して借用衝突を回避
        let schemes: Vec<TypeScheme> = self.scope.values().cloned().collect();
        for scheme in schemes {
            let mut body_vars = std::collections::HashSet::new();
            self.free_vars(scheme.body, &mut body_vars);
            // body にあるけど vars に量化されてないやつを取る
            for v in body_vars {
                if !scheme.vars.contains(&v) {
                    out.insert(v);
                }
            }
        }
        out
    }

    /// 型をスキームとして一般化する
    /// `free_vars(t) - env_free_vars()` を量化変数とする
    fn generalize(&mut self, t: TypeId) -> TypeScheme {
        let mut t_vars = std::collections::HashSet::new();
        self.free_vars(t, &mut t_vars);

        let env_vars = self.env_free_vars();

        // 型に出てくる Var のうち、環境に出てこないものだけ量化
        let quantified: Vec<TypeId> = t_vars
            .into_iter()
            .filter(|v| !env_vars.contains(v))
            .collect();

        TypeScheme {
            vars: quantified,
            body: t,
        }
    }

    /// 型のコピーを作りながら、subst に従って Var を置換する
    fn copy_with_subst(&mut self, id: TypeId, subst: &HashMap<TypeId, TypeId>) -> TypeId {
        let id = self.resolve(id);
        match self.db[id].clone() {
            DbEntry::Var => {
                if let Some(&new) = subst.get(&id) {
                    new
                } else {
                    id // 量化されていない Var はそのまま共有
                }
            }
            DbEntry::Concrete(_) => id, // 具象型はイミュータブルなので共有
            DbEntry::Function {
                params,
                return_type,
            } => {
                let new_params: Vec<TypeId> = params
                    .iter()
                    .map(|p| self.copy_with_subst(*p, subst))
                    .collect();
                let new_return = self.copy_with_subst(return_type, subst);
                let new_id = self.db.len();
                self.db.push(DbEntry::Function {
                    params: new_params,
                    return_type: new_return,
                });
                new_id
            }
            DbEntry::Symlink(_) => unreachable!("resolve should collapse Symlinks"),
        }
    }
}

pub fn type_check(statements: &[Statement]) -> Vec<TypeError> {
    let mut checker = TypeChecker::new();
    for stmt in statements {
        checker.visit_statement(stmt);
    }
    checker.errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unify_same_concrete() {
        let mut tc = TypeChecker::new();
        let a = tc.concrete("Number");
        let b = tc.concrete("Number");
        assert!(tc.unify(a, b));
        assert_eq!(tc.errors.len(), 0);
    }

    #[test]
    fn unify_different_concrete_fails() {
        let mut tc = TypeChecker::new();
        let a = tc.concrete("Number");
        let b = tc.concrete("String");
        assert!(!tc.unify(a, b));
        assert_eq!(tc.errors.len(), 1);
    }

    #[test]
    fn unify_var_with_concrete() {
        let mut tc = TypeChecker::new();
        let v = tc.fresh_var();
        let c = tc.concrete("Number");
        assert!(tc.unify(v, c));
        assert_eq!(tc.concrete_name(v), Some("Number".to_string()));
    }

    #[test]
    fn unify_two_vars_then_concrete() {
        // a と b を unify、その後 b を Number と unify
        // → a も Number になる
        let mut tc = TypeChecker::new();
        let a = tc.fresh_var();
        let b = tc.fresh_var();
        let n = tc.concrete("Number");
        assert!(tc.unify(a, b));
        assert!(tc.unify(b, n));
        assert_eq!(tc.concrete_name(a), Some("Number".to_string()));
    }

    #[test]
    fn unify_idempotent() {
        let mut tc = TypeChecker::new();
        let a = tc.concrete("Number");
        assert!(tc.unify(a, a));
    }

    #[test]
    fn type_check_simple_const() {
        let stmts = crate::compile("const x = 5;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_string_const() {
        let stmts = crate::compile(r#"const msg = "hello";"#);
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_return_with_value() {
        // return 文単独はパースできないので、関数内で確認するのは Step 6 以降に回す
        // ここでは型がエラーなく走ることだけ確認
        let stmts = crate::compile("const x = 1;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_add_numbers() {
        let stmts = crate::compile("const x = 1 + 2;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_add_strings() {
        let stmts = crate::compile(r#"const x = "a" + "b";"#);
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_add_mismatched() {
        let stmts = crate::compile(r#"const x = 1 + "hi";"#);
        let errors = type_check(&stmts);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Number"));
        assert!(errors[0].message.contains("String"));
    }

    #[test]
    fn type_check_multiply_numbers() {
        let stmts = crate::compile("const x = 2 * 3;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_multiply_string_fails() {
        let stmts = crate::compile(r#"const x = "a" * 2;"#);
        let errors = type_check(&stmts);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.message.contains("Number")));
    }

    #[test]
    fn type_check_chain() {
        // 1 + 2 + 3 → 全部 Number、エラーなし
        let stmts = crate::compile("const x = 1 + 2 + 3;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_ternary_ok() {
        let stmts = crate::compile("const x = true ? 1 : 2;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_ternary_test_not_boolean() {
        let stmts = crate::compile("const x = 1 ? 1 : 2;");
        let errors = type_check(&stmts);
        assert!(errors.iter().any(|e| e.message.contains("Boolean")));
    }

    #[test]
    fn type_check_ternary_branches_mismatch() {
        let stmts = crate::compile(r#"const x = true ? 1 : "hi";"#);
        let errors = type_check(&stmts);
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Number") && e.message.contains("String"))
        );
    }

    #[test]
    fn type_check_ternary_string_branches() {
        let stmts = crate::compile(r#"const x = true ? "a" : "b";"#);
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_array_ok() {
        let stmts = crate::compile("const xs = [1, 2, 3];");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_array_empty() {
        let stmts = crate::compile("const xs = [];");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_array_mixed() {
        let stmts = crate::compile(r#"const xs = [1, "two", 3];"#);
        let errors = type_check(&stmts);
        assert!(errors.iter().any(|e| e.message.contains("consistent")));
    }

    #[test]
    fn type_check_array_strings() {
        let stmts = crate::compile(r#"const xs = ["a", "b", "c"];"#);
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_arrow_function() {
        let stmts = crate::compile("const f = (x) => { return x; };");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_arrow_with_body() {
        let stmts = crate::compile("const inc = (x) => { return x + 1; };");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_call() {
        let stmts = crate::compile("const f = (x) => { return x; }; const r = f(5);");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_call_in_expression() {
        let stmts = crate::compile("const f = (x) => { return x; }; const r = f(1) + 2;");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_call_with_correct_arity() {
        let stmts = crate::compile("const add = (a, b) => { return a + b; }; const r = add(1, 2);");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn type_check_call_with_wrong_arity() {
        let stmts = crate::compile("const f = (x) => { return x; }; const r = f(1, 2);");
        let errors = type_check(&stmts);
        assert!(errors.iter().any(|e| e.message.contains("arity")));
    }

    #[test]
    fn type_check_call_with_wrong_arg_type() {
        let stmts = crate::compile("const inc = (x) => { return x * 2; }; const r = inc(\"hi\");");
        let errors = type_check(&stmts);
        assert!(!errors.is_empty());
    }

    #[test]
    fn type_check_call_chain() {
        let stmts = crate::compile("const inc = (x) => { return x + 1; }; const r = inc(inc(5));");
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn free_vars_concrete() {
        let mut tc = TypeChecker::new();
        let n = tc.concrete("Number");
        let mut set = std::collections::HashSet::new();
        tc.free_vars(n, &mut set);
        assert!(set.is_empty());
    }

    #[test]
    fn free_vars_single_var() {
        let mut tc = TypeChecker::new();
        let v = tc.fresh_var();
        let mut set = std::collections::HashSet::new();
        tc.free_vars(v, &mut set);
        assert_eq!(set.len(), 1);
        assert!(set.contains(&v));
    }

    #[test]
    fn free_vars_function() {
        let mut tc = TypeChecker::new();
        let t = tc.fresh_var();
        let u = tc.fresh_var();
        let n = tc.concrete("Number");
        // (T, Number) → U
        let f_id = tc.db.len();
        tc.db.push(DbEntry::Function {
            params: vec![t, n],
            return_type: u,
        });
        let mut set = std::collections::HashSet::new();
        tc.free_vars(f_id, &mut set);
        assert_eq!(set.len(), 2);
        assert!(set.contains(&t));
        assert!(set.contains(&u));
    }
    #[test]
    fn generalize_concrete_stays_monomorphic() {
        let mut tc = TypeChecker::new();
        let n = tc.concrete("Number");
        let scheme = tc.generalize(n);
        assert!(scheme.vars.is_empty());
        assert_eq!(scheme.body, n);
    }

    #[test]
    fn generalize_var_becomes_quantified() {
        let mut tc = TypeChecker::new();
        let v = tc.fresh_var();
        let scheme = tc.generalize(v);
        assert_eq!(scheme.vars, vec![v]);
    }

    #[test]
    fn generalize_skips_env_vars() {
        let mut tc = TypeChecker::new();
        // 環境に Var が1つ
        let outer = tc.fresh_var();
        tc.scope
            .insert("outer".to_string(), TypeScheme::monomorphic(outer));

        // 新しい関数型 (T) → outer を作って generalize
        let t = tc.fresh_var();
        let f_id = tc.db.len();
        tc.db.push(DbEntry::Function {
            params: vec![t],
            return_type: outer,
        });
        let scheme = tc.generalize(f_id);
        // outer は環境にあるので量化されない、T は量化される
        assert!(scheme.vars.contains(&t));
        assert!(!scheme.vars.contains(&outer));
    }

    #[test]
    fn let_polymorphism_id_with_different_types() {
        let stmts = crate::compile(
            "const id = (x) => { return x; };
         const a = id(5);
         const b = id(\"hi\");",
        );
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn let_polymorphism_id_then_bool() {
        let stmts = crate::compile(
            "const id = (x) => { return x; };
         const a = id(true);
         const b = id(1);",
        );
        assert_eq!(type_check(&stmts), vec![]);
    }

    #[test]
    fn monomorphic_function_still_constrains() {
        // inc は Number → Number に固定される
        let stmts = crate::compile(
            "const inc = (x) => { return x + 1; };
         const a = inc(\"hi\");", // ← エラー
        );
        let errors = type_check(&stmts);
        assert!(
            !errors.is_empty(),
            "Number-only function should reject String"
        );
    }

    #[test]
    fn env_aware_generalize() {
        // 外側 outer の x の型と、内側 inner の型が共有される
        // inner の x 部分は generalize で量化されない
        let stmts = crate::compile(
            "const outer = (x) => {
            const inner = (y) => { return x; };
            return inner(5);
         };",
        );
        // エラーが出ないことを確認
        assert_eq!(type_check(&stmts), vec![]);
    }
}
