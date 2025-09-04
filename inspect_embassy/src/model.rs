//! Contains types to model the memory layout of the future types of a particular program and to
//! store the value of those types in a running target.

use std::{collections::HashMap, fmt::Display};

use anyhow::Result;

use ddbug_parser::FileHash;

use task_pool::{TaskPool, TaskPoolValue};

use self::{future::FutureType, task_pool::HeaderLayout, ty::Type};

pub(crate) mod async_fn;
pub(crate) mod future;
pub(crate) mod task_pool;
pub(crate) mod ty;

/// Converts a namespace into a path separated by `::`.
fn namespace_to_path(namespace: &ddbug_parser::Namespace<'_>) -> String {
    let name = namespace.name().unwrap_or("<unknown>");
    match namespace.parent() {
        Some(parent) => {
            let mut path = namespace_to_path(parent);
            path.push_str("::");
            path.push_str(&name);
            path
        }
        None => name.to_owned(),
    }
}

/// Converts a namespace and a name into a path separated by `::`.
fn from_namespace_and_name(
    namespace: Option<&ddbug_parser::Namespace<'_>>,
    name: Option<&str>,
) -> String {
    let mut result = String::new();
    if let Some(namespace) = namespace {
        result.push_str(&namespace_to_path(namespace));
        result.push_str("::");
    }
    if let Some(type_name) = name {
        result.push_str(type_name);
    } else {
        result.push_str("<unknown>");
    }
    result
}

/// A location in the source code.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Source {
    pub(crate) path: String,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

impl Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (0, _) => write!(f, "{}", self.path),
            (line, 0) => write!(f, "{}:{}", self.path, line),
            (line, column) => write!(f, "{}:{}:{}", self.path, line, column),
        }
    }
}

impl Source {
    fn from_ddbug(source: &ddbug_parser::Source<'_>) -> Option<Self> {
        let path = match (source.directory(), source.file()?) {
            (_, "") => return None,
            (None | Some(""), file) => file.to_owned(),
            (Some(path), file) => format!("{path}/{file}"),
        };

        Some(Self {
            path,
            line: source.line(),
            column: source.column(),
        })
    }
}

/// The full model extracted from the debug data.
#[derive(Debug, Clone)]
pub(crate) struct DebugData {
    /// Address of the ends of the poll functions.
    ///
    /// Can be more than one because of the use of multiple executors or inlining.
    pub(crate) poll_done_addresses: Vec<u64>,

    pub(crate) future_types: HashMap<Type, FutureType>,
    pub(crate) task_pools: Vec<TaskPool>,
}

impl DebugData {
    pub(crate) fn from_object_file(path: String) -> Result<Self> {
        let file = ddbug_parser::File::parse(path)?;
        let file = file.file();
        let file_hash = FileHash::new(file);

        let mut future_types = HashMap::new();
        for unit in file.units() {
            for unit_type in unit.types() {
                if let Some(future) = FutureType::from_ddbug_type(unit_type, &file_hash)? {
                    let ty = Type::from_ddbug_type(unit_type, &file_hash);
                    future_types.insert(ty, future);
                }
            }
        }

        let header_layout = HeaderLayout::from_ddbug_data(&file_hash)?;

        let mut task_pools = HashMap::new();
        for unit in file.units() {
            for unit_var in unit.variables() {
                if let Some(task_pool) =
                    TaskPool::from_ddbug_var(unit_var, &future_types, &header_layout, &file_hash)?
                {
                    task_pools.insert(task_pool.path.clone(), task_pool);
                }
            }
        }
        let mut task_pools = task_pools.into_values().collect::<Vec<_>>();
        task_pools.sort_unstable_by_key(|task| std::cmp::Reverse(task.async_fn_type.total_size));

        let poll_done_addresses = find_poll_function_addresses(&file_hash);
        if poll_done_addresses.is_empty() {
            log::warn!(
                "Could't not find the poll function, manualy break the target to update the display"
            );
        }

        Ok(Self {
            poll_done_addresses,
            task_pools,
            future_types,
        })
    }

    pub(crate) fn get_taskpool_value(&self, task_pool: &TaskPool, bytes: &[u8]) -> TaskPoolValue {
        TaskPoolValue::new(task_pool, bytes, &self.future_types)
    }
}

fn find_poll_function_addresses(file_hash: &FileHash) -> Vec<u64> {
    // Searches for a function with the path: embassy_executor::raw::{impl#9}::poll::{closure#0}
    // where #9 can be replaced with anything.
    let poll_function = 'main: {
        for unit in file_hash.file.units() {
            for unit_fn in unit.functions() {
                if let Some(name) = unit_fn.name()
                    && name.contains("{closure")
                    && let Some(linkage_name) = unit_fn.linkage_name()
                    && linkage_name.contains("SyncExecutor")
                    && let Some(namespace) = unit_fn.namespace()
                    && let namespace = namespace_to_path(namespace)
                    && namespace.starts_with("embassy_executor::raw")
                    && namespace.ends_with("poll")
                {
                    if let [range] = unit_fn.ranges() {
                        return vec![range.end - 4];
                    } else if unit_fn.is_inline() {
                        break 'main unit_fn;
                    }
                }
            }
        }
        return Vec::new();
    };

    // Poll function got inlined, search all functions for where it ended up.
    let mut addresses = Vec::new();
    for unit in file_hash.file.units() {
        for unit_fn in unit.functions() {
            let details = unit_fn.details(file_hash);

            for inlined_function in details.inlined_functions() {
                find_function_in_inlined(
                    poll_function,
                    inlined_function,
                    file_hash,
                    &mut addresses,
                );
            }
        }
    }

    return addresses;
}

/// Recursively look for all locations the given function is inlined into the given inlined_function.
/// Adds the found address to the found_addresses.
fn find_function_in_inlined(
    function: &ddbug_parser::Function,
    inlined_function: &ddbug_parser::InlinedFunction,
    file_hash: &FileHash,
    found_addresses: &mut Vec<u64>,
) {
    if let Some(resolved_inlined_function) = inlined_function.abstract_origin(file_hash)
        && ddbug_parser::Function::<'_>::cmp_id(
            file_hash,
            resolved_inlined_function,
            file_hash,
            function,
        )
        .is_eq()
    {
        for range in inlined_function.ranges() {
            if range.begin == 0 {
                // Strange bug where every inlined function also has a range staring at 0.
                // Just ignoring it here.
                continue;
            }
            found_addresses.push(range.end);
        }
    }

    for inlined_function in inlined_function.inlined_functions() {
        find_function_in_inlined(function, inlined_function, file_hash, found_addresses);
    }
}
