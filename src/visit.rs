use crate::ast_span::{Expression, Parameter, Statement, TypeAnnotation};

pub trait Visit {
    fn visit_statement(&mut self, stmt: &Statement) {
        walk_statement(self, stmt);
    }
    fn visit_expression(&mut self, expr: &Expression) {
        walk_expression(self, expr);
    }
    fn visit_type_annotation(&mut self, ty: &TypeAnnotation) {
        walk_type_annotation(self, ty);
    }
    fn visit_parameter(&mut self, param: &Parameter) {
        walk_parameter(self, param);
    }
}

pub fn walk_statement<V: Visit + ?Sized>(v: &mut V, stmt: &Statement) {
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

pub fn walk_expression<V: Visit + ?Sized>(v: &mut V, expr: &Expression) {
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

pub fn walk_type_annotation<V: Visit + ?Sized>(v: &mut V, ty: &TypeAnnotation) {
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

pub fn walk_parameter<V: Visit + ?Sized>(v: &mut V, param: &Parameter) {
    if let Some(ty) = &param.type_annotation {
        v.visit_type_annotation(ty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_result::parse_result;
    use crate::tokenize_span::tokenize_span;

    struct IdentifierCollector {
        names: Vec<String>,
    }

    impl Visit for IdentifierCollector {
        fn visit_expression(&mut self, expr: &Expression) {
            if let Expression::Identifier { name, .. } = expr {
                self.names.push(name.clone());
            }
            walk_expression(self, expr);
        }
    }

    #[test]
    fn collects_identifiers() {
        let stmts = parse_result(tokenize_span("const x = a + b * c;")).unwrap();
        let mut collector = IdentifierCollector { names: Vec::new() };
        for stmt in &stmts {
            collector.visit_statement(stmt);
        }
        assert_eq!(collector.names, vec!["a", "b", "c"]);
    }

    #[test]
    fn collects_through_call_and_arrow() {
        let stmts =
            parse_result(tokenize_span("const f = (z) => { return z + g(x, y); };")).unwrap();
        let mut c = IdentifierCollector { names: Vec::new() };
        for s in &stmts {
            c.visit_statement(s);
        }
        assert_eq!(c.names, vec!["z", "g", "x", "y"]);
    }

    #[test]
    fn collects_in_conditional_and_array() {
        let stmts = parse_result(tokenize_span("const xs = [a ? b : c, d];")).unwrap();
        let mut c = IdentifierCollector { names: Vec::new() };
        for s in &stmts {
            c.visit_statement(s);
        }
        assert_eq!(c.names, vec!["a", "b", "c", "d"]);
    }

    struct TypeNameCollector {
        type_names: Vec<String>,
    }

    impl Visit for TypeNameCollector {
        fn visit_type_annotation(&mut self, ty: &TypeAnnotation) {
            if let TypeAnnotation::Named { name, .. } = ty {
                self.type_names.push(name.clone());
            }
            walk_type_annotation(self, ty);
        }
    }

    #[test]
    fn collects_type_names_in_arrow_function() {
        let stmts = parse_result(tokenize_span(
            "const f = (x: number, y: string): boolean => { return true; };",
        ))
        .unwrap();
        let mut c = TypeNameCollector {
            type_names: Vec::new(),
        };
        for s in &stmts {
            c.visit_statement(s);
        }
        assert_eq!(c.type_names, vec!["number", "string", "boolean"]);
    }
}
