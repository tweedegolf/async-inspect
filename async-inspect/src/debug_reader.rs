use std::{collections::HashMap, path::Path};

use dwarf_reader::FutureType;

/// Rust uses a diffrent name in the debug info and `std::any::get_name` so only
/// keeping the path (wich includes the async function name so it should still be unique).
///
/// For example:
/// - Debug: `simple::foo::{closure_env#0}<simple::foo_2::{closure_env#0}>`
/// - get_name: `simple::foo::{{closure}}<simple::foo_2::{{closure}}>`
/// - output from this function: `simple::foo::_<simple::foo_2::_>`
pub(crate) fn normalize_name(mut name: &str) -> String {
    let mut nor = String::new();

    enum State {
        Path,
        Virtual(u32),
    }
    let mut state = State::Path;

    while let Some(idx) = name.find(&['{', '}']) {
        match (&name[idx..=idx], state) {
            ("{", State::Path) => {
                nor.push_str(&name[..idx]);
                state = State::Virtual(1);
            }
            ("{", State::Virtual(level)) => {
                state = State::Virtual(level + 1);
            }
            ("}", State::Path) => {
                unreachable!()
            }
            ("}", State::Virtual(level)) => {
                if level == 1 {
                    nor.push('_');
                    state = State::Path;
                } else {
                    state = State::Virtual(level - 1);
                }
            }
            _ => unreachable!(),
        }
        name = &name[idx + 1..];
    }
    nor.push_str(&name);

    nor
}

pub(crate) struct DebugData {
    pub(crate) future_types: HashMap<String, FutureType>,
}

impl DebugData {
    pub(crate) fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let future_types = dwarf_reader::from_file(path).unwrap();

        let future_types = future_types
            .into_iter()
            .map(|t| (normalize_name(&t.path), t))
            .collect();

        Self { future_types }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_paths_eq() {
        let output_1 =
            normalize_name("simple::foo::{closure_env#0}<simple::foo_2::{closure_env#0}>");
        let output_2 = normalize_name("simple::foo::{{closure}}<simple::foo_2::{{closure}}>");

        assert_eq!(output_1, output_2);
        assert_eq!(output_1, "simple::foo::_<simple::foo_2::_>");
    }
}
