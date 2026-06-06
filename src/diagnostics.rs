use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term::{
    self,
    termcolor::{ColorChoice, StandardStream},
};

use crate::parse_result::ParseError;

pub fn emit_parse_error(source: &str, err: &ParseError) {
    let mut files = SimpleFiles::new();
    let file_id = files.add("input.ts", source);

    let diagnostic = Diagnostic::error()
        .with_message(&err.message)
        .with_labels(vec![
            Label::primary(file_id, err.span.start..err.span.end).with_message(&err.message),
        ]);

    let writer = StandardStream::stderr(ColorChoice::Auto);
    let config = term::Config::default();
    term::emit(&mut writer.lock(), &config, &files, &diagnostic).unwrap();
}
