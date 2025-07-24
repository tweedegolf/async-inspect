use std::path::Path;
pub use type_parser::{FutureType, Layout, Member, State};

mod type_parser;

pub fn from_file<P: AsRef<Path>>(path: P) -> ddbug_parser::Result<Vec<type_parser::FutureType>> {
    let path = path.as_ref();

    // TODO: fork(?) ddbug_parser to take a path instead.
    let ctx = ddbug_parser::File::parse(path.display().to_string())?;
    let file = ctx.file();

    type_parser::parse_file(file)
}
