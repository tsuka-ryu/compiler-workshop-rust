use crate::ast_span::{Expression, Parameter, Statement, TypeAnnotation};

pub trait VisitMut {
    fn visit_statement(&mut self, stmt: &mut Statement) {
        walk_statement(self, stmt);
    }
    fn visit_expression(&mut self, expr: &mut Expression) {
        walk_expression(self, expr);
    }
    fn visit_type_annotation(&mut self, ty: &mut TypeAnnotation) {
        walk_type_annotation(self, ty);
    }
    fn visit_parameter(&mut self, param: &mut Parameter) {
        walk_parameter(self, param);
    }
}

pub fn walk_statement<V: VisitMut + ?Sized>(v: &mut V, stmt: &mut Statement) {
    match stmt {
        Statement::ConstDeclaration {
            type_annotation,
            init,
            ..
        } => {
            if let Some(ty) = type_annotation {
                v.visit_type_annotation(ty);
            }
            v.visit_expression(init);
        }
        Statement::Return { argument, .. } => {
            if let Some(arg) = argument {
                v.visit_expression(arg);
            }
        }
    }
}

pub fn walk_expression<V: VisitMut + ?Sized>(v: &mut V, expr: &mut Expression) {
    match expr {
        // 子なしは何もしない
        Expression::Number { .. }
        | Expression::String { .. }
        | Expression::Boolean { .. }
        | Expression::Identifier { .. } => {}
        // 子ありは、子をvisit_*で辿る
        Expression::Binary { left, right, .. } => {
            v.visit_expression(left);
            v.visit_expression(right);
        }
        Expression::Conditional {
            test,
            consequent,
            alternate,
            ..
        } => {
            v.visit_expression(test);
            v.visit_expression(consequent);
            v.visit_expression(alternate);
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            v.visit_expression(callee);
            for arg in arguments {
                v.visit_expression(arg);
            }
        }
        Expression::Array { elements, .. } => {
            for elem in elements {
                v.visit_expression(elem);
            }
        }
        Expression::Member { object, index, .. } => {
            v.visit_expression(object);
            v.visit_expression(index);
        }
        Expression::ArrowFunction {
            params,
            return_type,
            body,
            ..
        } => {
            for param in params {
                v.visit_parameter(param);
            }
            if let Some(ty) = return_type {
                v.visit_type_annotation(ty);
            }
            for stmt in body {
                v.visit_statement(stmt);
            }
        }
    }
}

pub fn walk_type_annotation<V: VisitMut + ?Sized>(v: &mut V, ty: &mut TypeAnnotation) {
    match ty {
        TypeAnnotation::Named { .. } => {}
        TypeAnnotation::Array { element, .. } => {
            v.visit_type_annotation(element);
        }
        TypeAnnotation::Function {
            params,
            return_type,
            ..
        } => {
            for param in params {
                v.visit_parameter(param);
            }
            v.visit_type_annotation(return_type);
        }
    }
}

pub fn walk_parameter<V: VisitMut + ?Sized>(v: &mut V, param: &mut Parameter) {
    if let Some(ty) = &mut param.type_annotation {
        v.visit_type_annotation(ty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_result::parse_result;
    use crate::tokenize_span::tokenize_span;

    struct IdentifierRenamer {
        suffix: &'static str,
    }

    impl VisitMut for IdentifierRenamer {
        fn visit_expression(&mut self, expr: &mut Expression) {
            if let Expression::Identifier { name, .. } = expr {
                name.push_str(self.suffix);
            }
            walk_expression(self, expr);
        }
    }

    fn collect_identifiers(stmts: &[Statement]) -> Vec<String> {
        use crate::visit::{walk_expression as walk, Visit};
        struct Collector {
            names: Vec<String>,
        }
        impl Visit for Collector {
            fn visit_expression(&mut self, expr: &Expression) {
                if let Expression::Identifier { name, .. } = expr {
                    self.names.push(name.clone());
                }
                walk(self, expr);
            }
        }
        let mut c = Collector { names: Vec::new() };
        for s in stmts {
            c.visit_statement(s);
        }
        c.names
    }

    #[test]
    fn renames_identifiers() {
        let mut stmts = parse_result(tokenize_span("const x = a + b * c;")).unwrap();
        let mut r = IdentifierRenamer { suffix: "_v2" };
        for stmt in &mut stmts {
            r.visit_statement(stmt);
        }
        assert_eq!(
            collect_identifiers(&stmts),
            vec!["a_v2", "b_v2", "c_v2"]
        );
    }

    #[test]
    fn renames_through_call_and_arrow() {
        let mut stmts =
            parse_result(tokenize_span("const f = (z) => { return z + g(x, y); };")).unwrap();
        let mut r = IdentifierRenamer { suffix: "!" };
        for s in &mut stmts {
            r.visit_statement(s);
        }
        assert_eq!(
            collect_identifiers(&stmts),
            vec!["z!", "g!", "x!", "y!"]
        );
    }

    struct ConstantFolder;

    impl VisitMut for ConstantFolder {
        fn visit_expression(&mut self, expr: &mut Expression) {
            // 子を先に畳む (post-order)
            walk_expression(self, expr);
            if let Expression::Binary {
                left,
                op,
                right,
                span,
            } = expr
            {
                if let (
                    Expression::Number { value: l, .. },
                    Expression::Number { value: r, .. },
                ) = (left.as_ref(), right.as_ref())
                {
                    use crate::ast_span::BinaryOp;
                    let folded = match op {
                        BinaryOp::Add => l + r,
                        BinaryOp::Multiply => l * r,
                    };
                    *expr = Expression::Number {
                        value: folded,
                        span: *span,
                    };
                }
            }
        }
    }

    #[test]
    fn folds_constants() {
        // parse_result は precedence 未実装の左結合なので (1+2)*3 = 9 になる。
        // ここで確認したいのは「ネストした畳み込みが post-order で正しく回ること」
        let mut stmts = parse_result(tokenize_span("const x = 1 + 2 * 3;")).unwrap();
        let mut f = ConstantFolder;
        for s in &mut stmts {
            f.visit_statement(s);
        }
        if let Statement::ConstDeclaration { init, .. } = &stmts[0] {
            assert!(matches!(init, Expression::Number { value: 9, .. }));
        } else {
            panic!("expected ConstDeclaration");
        }
    }
}