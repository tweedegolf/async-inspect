use std::collections::HashMap;

use ddbug_parser::{File, FileHash, Result, StructType, TypeKind};

// Defined here: https://github.com/rust-lang/rust/blob/a9fb6103b05c6ad6eee6bed4c0bb5a2e8e1024c6/compiler/rustc_codegen_ssa/src/debuginfo/type_names.rs#L566
const FUTURE_TYPE_NAMES: &[&str] = &[
    "gen_block",
    "gen_closure",
    "gen_fn",
    "async_block",
    "async_closure",
    "async_fn",
    "async_gen_block",
    "async_gen_closure",
    "async_gen_fn",
];

pub fn parse_file(file: &File) -> Result<Vec<FutureType>> {
    let file_hash = FileHash::new(file);

    let mut future_types = HashMap::new();

    for unit in file.units() {
        for unit_type in unit.types() {
            if let Some(future) = FutureType::from_ddbug_type(unit_type, &file_hash)? {
                future_types.insert(future.path.clone(), future);
            }
        }
    }

    let mut future_types = future_types.into_values().collect::<Vec<_>>();
    future_types.sort_unstable_by(|a, b| b.layout.total_size.cmp(&a.layout.total_size));

    Ok(future_types)
}

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

fn type_to_string(ty: &ddbug_parser::Type, file_hash: &FileHash) -> String {
    match ty.kind() {
        TypeKind::Void => String::from("void"),
        TypeKind::Base(base_type) => base_type.name().unwrap_or("<unknown>").to_string(),
        TypeKind::Def(type_def) => from_namespace_and_name(type_def.namespace(), type_def.name()),
        TypeKind::Struct(struct_type) => {
            from_namespace_and_name(struct_type.namespace(), struct_type.name())
        }
        TypeKind::Union(union_type) => {
            from_namespace_and_name(union_type.namespace(), union_type.name())
        }
        TypeKind::Enumeration(enumeration_type) => {
            from_namespace_and_name(enumeration_type.namespace(), enumeration_type.name())
        }
        TypeKind::Array(array_type) => {
            let inner = array_type
                .element_type(file_hash)
                .map(|inner| type_to_string(&inner, file_hash))
                .unwrap_or_else(|| String::from("<unknown>"));

            let counts = match array_type.counts().collect::<Vec<_>>().as_slice() {
                [] => String::new(),
                [None] => String::new(),
                other => {
                    let mut result = String::new();
                    for c in other {
                        result.push_str("; ");
                        match c {
                            Some(c) => {
                                result.push_str(&c.to_string());
                            }
                            None => result.push_str("; ?"),
                        }
                    }
                    result
                }
            };
            format!("[{inner}{counts}]")
        }
        TypeKind::Function(function_type) => {
            todo!()
        }
        TypeKind::Unspecified(unspecified_type) => {
            from_namespace_and_name(unspecified_type.namespace(), unspecified_type.name())
        }
        TypeKind::PointerToMember(pointer_to_member_type) => {
            let inner = pointer_to_member_type
                .member_type(file_hash)
                .map(|inner| type_to_string(&inner, file_hash))
                .unwrap_or_else(|| String::from("<unknown>"));
            format!("*{inner}")
        }
        TypeKind::Modifier(type_modifier) => {
            let inner = type_modifier
                .ty(file_hash)
                .map(|inner| type_to_string(&inner, file_hash))
                .unwrap_or_else(|| String::from("<unknown>"));

            let modifier = match type_modifier.kind() {
                ddbug_parser::TypeModifierKind::Pointer => "* ",
                ddbug_parser::TypeModifierKind::Reference => "& ",
                ddbug_parser::TypeModifierKind::Const => "const ",
                ddbug_parser::TypeModifierKind::Packed => "packed ",
                ddbug_parser::TypeModifierKind::Volatile => "volatile ",
                ddbug_parser::TypeModifierKind::Restrict => "",
                ddbug_parser::TypeModifierKind::Shared => "",
                ddbug_parser::TypeModifierKind::RvalueReference => "",
                ddbug_parser::TypeModifierKind::Atomic => "",
                ddbug_parser::TypeModifierKind::Other => "",
            };
            format!("{modifier}{inner}")
        }
        TypeKind::Subrange(subrange_type) => todo!(),
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Member {
    pub name: String,
    pub type_name: String,
    /// Offset from the start of the future struct
    pub offset: u64,
    pub size: u64,
}

impl Member {
    fn from_ddbug_member(member: &ddbug_parser::Member<'_>, file_hash: &FileHash) -> Result<Self> {
        let name = member.name().ok_or("members should have names")?.to_owned();

        let type_name = match member.ty(file_hash) {
            Some(ty) => type_to_string(&ty, file_hash),
            None => "<unknown>".to_owned(),
        };

        let offset = member.bit_offset() / 8;
        let size = member
            .bit_size(file_hash)
            .ok_or("member should have known sizes")?
            / 8;

        Ok(Self {
            name,
            type_name,
            offset,
            size,
        })
    }
}

#[derive(Debug, Clone)]
pub struct State {
    pub discriminant_value: u64,
    pub active_members: Vec<usize>,

    pub awaitee: Option<Member>,
    pub name: String,
}

impl State {
    fn from_ddbug_variant(
        variant: &ddbug_parser::Variant<'_>,
        active_members: Vec<usize>,
        awaitee: Option<Member>,
    ) -> Result<Self> {
        let state_name = variant.name().unwrap_or("<unknown>").to_owned();
        let Some(discriminant_value) = variant.discriminant_value() else {
            return Err("future type varaints should always have discriminant values".into());
        };

        Ok(State {
            name: state_name,
            discriminant_value,
            active_members,
            awaitee,
        })
    }
}

/// The layout of a future type
#[derive(Debug, Clone)]
pub struct Layout {
    pub members: Vec<Member>,

    pub state_member: Member,

    pub total_size: u64,

    pub states: Vec<State>,
}

impl Layout {
    /// Get the layout of a Future type from the ddbug_type, ddbug_type should always be describing
    /// a future type.
    fn from_ddbug_type(ddbug_type: &StructType<'_>, file_hash: &FileHash) -> Result<Self> {
        let [variant_part] = ddbug_type.variant_parts() else {
            return Err("Future types should always have a single variant part".into());
        };

        let mut members = Vec::new();
        let mut member_to_id = HashMap::<Member, usize>::new();

        let mut states = Vec::new();

        for variant in variant_part.variants() {
            let mut active_members = Vec::new();
            let mut awaitee = None;

            for member in variant.members() {
                let member = Member::from_ddbug_member(member, file_hash)?;

                if member.name == "__awaitee" {
                    if awaitee.is_some() {
                        return Err("future type contains variants with multiple awaitees".into());
                    }
                    awaitee = Some(member);
                    continue;
                }

                if member.size == 0 {
                    continue;
                }

                let id = match member_to_id.get(&member) {
                    Some(other_id) => *other_id,
                    None => {
                        let member_id = members.len();
                        member_to_id.insert(member.clone(), member_id);
                        members.push(member);
                        member_id
                    }
                };

                // Rust somtimes ouputs the same field multiple times
                if !active_members.contains(&id) {
                    active_members.push(id);
                }
            }

            states.push(State::from_ddbug_variant(variant, active_members, awaitee)?);
        }

        let [state_member] = ddbug_type.members() else {
            return Err("Future types should always have a member".into());
        };
        let state_member = Member::from_ddbug_member(state_member, file_hash)?;
        if state_member.name != "__state" {
            return Err("Future types should always have a member named __state".into());
        }

        let Some(total_size) = ddbug_type.byte_size() else {
            return Err("Future types should have a size".into());
        };

        Ok(Self {
            members,

            state_member,

            total_size,

            states,
        })
    }

    /// Sort the members from smalles to biggest offset while kepping all id refrences intact
    fn sort_members_by_offset(&mut self) {
        let mut old_ids = (0..self.members.len()).collect::<Vec<_>>();
        old_ids.sort_unstable_by_key(|id| self.members[*id].offset);

        let mut new_ids = vec![0; old_ids.len()];
        let mut new_members = vec![Member::default(); self.members.len()];
        for (new_id, old_id) in old_ids.iter().enumerate() {
            std::mem::swap(&mut self.members[*old_id], &mut new_members[new_id]);
            new_ids[*old_id] = new_id;
        }
        self.members = new_members;

        for state in &mut self.states {
            for active_member in &mut state.active_members {
                *active_member = new_ids[*active_member];
            }
            state.active_members.sort_unstable();
        }
    }
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut member_pos = Vec::new();

        let mut members_line1 = String::from("          | ");
        let mut members_line2 = String::from("          | ");

        let mut add_col = |line1: &str, line2: &str| {
            let max_size = line1.len().max(line2.len());

            let col = members_line1.len();
            members_line1.push_str(line1);
            members_line2.push_str(line2);

            members_line1.push_str(&" ".repeat(max_size - line1.len()));
            members_line2.push_str(&" ".repeat(max_size - line2.len()));

            members_line1.push_str(" | ");
            members_line2.push_str(" | ");

            (col, max_size)
        };

        // let mut byte_offset = 0;
        let mut add_member = |member: &Member| {
            // if member.offset != byte_offset {
            //     add_col(
            //         "<padding>",
            //         &format!("{} bytes", member.offset - byte_offset),
            //     );
            //     byte_offset = member.offset;
            // }

            // byte_offset += member.size;

            add_col(
                &format!("{}: {}", member.name, member.type_name),
                &format!("{}[{}]", member.offset, member.size),
            )
        };

        for member in &self.members {
            let pos = add_member(member);
            member_pos.push(pos);
        }
        let state_pos = add_member(&self.state_member);

        let awaitee_pos = add_col("awaitee", "");

        writeln!(f, "{members_line1}")?;
        writeln!(f, "{members_line2}")?;
        writeln!(f, "")?;

        for state in &self.states {
            write!(f, "{}", &state.name)?;
            let mut current_col = state.name.len();

            for active_members in &state.active_members {
                let (col, len) = member_pos[*active_members];

                write!(f, "{}", " ".repeat(col - current_col))?;
                current_col = col;
                write!(f, "{}", "-".repeat(len))?;
                current_col += len;
            }

            write!(f, "{}", " ".repeat(state_pos.0 - current_col))?;
            let discriminant = state.discriminant_value.to_string();
            write!(f, "{}", discriminant)?;
            write!(f, "{}", " ".repeat(state_pos.1 - discriminant.len()))?;
            current_col = state_pos.0 + state_pos.1;

            match &state.awaitee {
                Some(awaitee) => {
                    write!(f, "{}", " ".repeat(awaitee_pos.0 - current_col))?;
                    writeln!(
                        f,
                        "{}[{}] {}",
                        awaitee.offset, awaitee.size, awaitee.type_name
                    )?;
                }
                None => {
                    writeln!(f, "")?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FutureType {
    pub path: String,
    pub layout: Layout,
}

impl FutureType {
    fn from_ddbug_type(
        ddbug_type: &ddbug_parser::Type<'_>,
        file_hash: &FileHash,
    ) -> Result<Option<Self>> {
        let TypeKind::Struct(struct_type) = ddbug_type.kind() else {
            return Ok(None);
        };

        let Some(struct_name) = struct_type.name() else {
            return Ok(None);
        };

        // The rust compiler gives generated Future types names of the form `{async_fn#0}<T,K>`
        // except on msvc platforms where it uses `async_fn$0<T, K>`.
        let is_future_type = FUTURE_TYPE_NAMES.iter().any(|future_name| {
            struct_name.starts_with(future_name) || struct_name[1..].starts_with(future_name)
        });

        if !is_future_type {
            return Ok(None);
        }

        let namespace = struct_type
            .namespace()
            .ok_or("future types should always be part of a namespace")?;
        let mut path = namespace_to_path(namespace);
        path.push_str("::");
        path.push_str(struct_name);

        let mut layout = Layout::from_ddbug_type(struct_type, file_hash)?;
        layout.sort_members_by_offset();

        Ok(Some(Self { path, layout }))
    }
}

impl std::fmt::Display for FutureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.path)?;
        writeln!(f, "{}", self.layout)
    }
}
