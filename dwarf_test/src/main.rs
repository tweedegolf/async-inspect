use std::env;

mod type_parser;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();
    if args.len() != 2 {
        println!("Usage: {} <file>", args.next().unwrap());
        return Ok(());
    }
    args.next().unwrap();
    let path = args.next().unwrap();

    let ctx = ddbug_parser::File::parse(path)?;
    let file = ctx.file();

    let future_types = type_parser::parse_file(file)?;
    for future_type in future_types {
        println!("{future_type}");
    }

    Ok(())
}
