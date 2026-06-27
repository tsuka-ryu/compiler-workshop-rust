//! javascript-parser-in-rust を読みながら作る JS パーサー。
//! 詳細は docs/js-parser-roadmap.md を参照。
//!
//! 各章のファイルができたらここに `pub mod lexer;` のように足していく。

pub mod ast;
pub mod lexer;
pub mod parser;
