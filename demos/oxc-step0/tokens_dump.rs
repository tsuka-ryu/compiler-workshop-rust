#![expect(clippy::print_stdout)]
//! Dump the collected token stream of a file.
//!
//! ```bash
//! cargo run -p oxc_parser --example tokens_dump -- file.ts
//! ```

use std::{fs, path::Path};

use oxc_allocator::Allocator;
use oxc_parser::{Parser, config::TokensParserConfig};
use oxc_span::SourceType;

fn main() -> Result<(), String> {
    let name = std::env::args().nth(1).ok_or("usage: tokens_dump <file>")?;
    let path = Path::new(&name);
    let source_text = fs::read_to_string(path).map_err(|_| format!("Missing '{name}'"))?;
    let source_type = SourceType::from_path(path).unwrap();

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source_text, source_type)
        .with_config(TokensParserConfig)
        .parse();

    for token in &ret.tokens {
        let text = &source_text[token.start() as usize..token.end() as usize];
        println!("{:>3}..{:<3} {:<16} {text:?}", token.start(), token.end(), format!("{:?}", token.kind()));
    }
    Ok(())
}
