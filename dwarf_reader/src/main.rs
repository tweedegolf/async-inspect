use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args();
    if args.len() != 2 {
        println!("Usage: {} <file>", args.next().unwrap());
        return Ok(());
    }
    args.next().unwrap();
    let path = args.next().unwrap();

    let future_types = dwarf_reader::from_file(path)?;
    for future_type in future_types {
        println!("{future_type}");
    }

    Ok(())
}
