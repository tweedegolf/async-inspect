use super::{
    async_fn::{AsyncFnType, AsyncFnValue},
    namespace_to_path,
};

use ddbug_parser::FileHash;

#[derive(Debug, Clone)]
pub(crate) struct TaskPool {
    pub(crate) path: String,

    // Address where the bytes are stored.
    pub(crate) address: u64,
    // Amount of bytes for the whole pool
    pub(crate) size: u64,

    // Maximum number of thask this pool can hold.
    pub(crate) number_of_tasks: usize,

    // The async fn type this pool stores
    pub(crate) async_fn_type: AsyncFnType,
}

impl TaskPool {
    pub(crate) fn find_taks_pool<'a>(
        task_name: &str,
        file_hash: &'a FileHash<'a>,
    ) -> Option<&'a ddbug_parser::StructType<'a>> {
        let task_pool_name_prefix = String::from("TaskPool<") + task_name;

        for unit in file_hash.file.units() {
            for unit_type in unit.types() {
                if let ddbug_parser::TypeKind::Struct(struct_type) = unit_type.kind() {
                    if let Some(name) = struct_type.name()
                        && name.starts_with(&task_pool_name_prefix)
                    {
                        return Some(struct_type);
                    }
                }
            }
        }

        return None;
    }

    // TODO: make this work when embassy is compiled with nightly
    pub(crate) fn from_ddbug_var(
        unit_var: &ddbug_parser::Variable<'_>,
        async_fn_types: &Vec<AsyncFnType>,
        file_hash: &FileHash<'_>,
    ) -> Option<Self> {
        if unit_var.name()? != "POOL" {
            return None;
        }
        let ty = unit_var.ty(file_hash)?;
        match ty.kind() {
            ddbug_parser::TypeKind::Struct(struct_type) => {
                if !struct_type.name()?.starts_with("TaskPoolHolder") {
                    return None;
                }
            }
            _ => return None,
        }

        let namespace = unit_var.namespace()?;

        // The task macro generates a namesapce with the name of the function, so the path generated
        // from only the namespaces will actually end in the name of the original task function.
        let path = namespace_to_path(namespace);
        let task_name = namespace_to_path(namespace.parent()?);
        let task_name = task_name + "::__" + namespace.name()? + "_task";

        let address = unit_var.address()?;
        let size = unit_var.byte_size(file_hash)?;

        let task_pool_type = Self::find_taks_pool(&task_name, file_hash)?;
        let [task_pool_member] = task_pool_type.members() else {
            return None;
        };
        let number_of_tasks = match task_pool_member.ty(file_hash)?.kind() {
            ddbug_parser::TypeKind::Array(array_type) => array_type.counts().next()??,
            _ => return None,
        } as usize;

        let async_fn_type = async_fn_types
            .iter()
            .find(|ty| ty.path.starts_with(&task_name))?
            .clone();

        Some(Self {
            path,
            address,
            size,
            number_of_tasks,
            async_fn_type,
        })
    }
}

#[derive(Debug)]
pub(crate) struct TaskPoolValue {
    pub(crate) task_pool: TaskPool,

    pub(crate) async_fn_values: Vec<AsyncFnValue>,
}

impl TaskPoolValue {
    pub(crate) fn new(task_pool: &TaskPool, bytes: &[u8], async_fns: &[AsyncFnType]) -> Self {
        assert_eq!(bytes.len() as u64, task_pool.size);
        let mut async_fn_values = Vec::new();

        let len_single_task = task_pool.size / task_pool.number_of_tasks as u64;

        for task in 0..task_pool.number_of_tasks {
            let bytes_offset = len_single_task + task as u64 * len_single_task
                - task_pool.async_fn_type.layout.total_size;
            let bytes = &bytes[bytes_offset as usize..];

            async_fn_values.push(AsyncFnValue::new(
                &task_pool.async_fn_type,
                bytes,
                async_fns,
            ))
        }

        Self {
            task_pool: task_pool.clone(),
            async_fn_values,
        }
    }
}
