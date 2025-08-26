use std::collections::HashMap;

use ddbug_parser::{FileHash, Result, StructType, TypeKind};

use super::{
    Source,
    future::{FutureType, FutureValue},
    ty::Type,
};

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

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) ty: Type,
    /// Offset from the start of the future struct
    pub(crate) offset: u64,
    pub(crate) size: u64,
}

impl Member {
    fn from_ddbug_member(member: &ddbug_parser::Member<'_>, file_hash: &FileHash) -> Result<Self> {
        let name = member.name().ok_or("members should have names")?.to_owned();

        let ty = Type::from_maybe_ddbug_type(member.ty(file_hash), file_hash);

        let offset = member.bit_offset() / 8;
        let size = member
            .bit_size(file_hash)
            .ok_or("member should have known sizes")?
            / 8;

        Ok(Self {
            name,
            ty,
            offset,
            size,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub(crate) discriminant_value: u64,
    pub(crate) active_members: Vec<usize>,

    pub(crate) awaitee: Option<Member>,
    pub(crate) name: String,
    pub(crate) source: Option<Source>,
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
            source: Source::from_ddbug(variant.source()),
        })
    }
}

/// The layout of a future type
#[derive(Debug, Clone)]
pub(crate) struct AsyncFnType {
    pub(crate) members: Vec<Member>,

    pub(crate) state_member: Member,

    pub(crate) total_size: u64,

    pub(crate) states: Vec<State>,
}

impl AsyncFnType {
    pub(crate) fn from_ddbug_type(
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

        Ok(Some(Self::from_ddbug_struct(struct_type, file_hash)?))
    }

    /// Get the layout of a Future type from the ddbug_type, ddbug_type should always be describing
    /// a future type.
    fn from_ddbug_struct(ddbug_type: &StructType<'_>, file_hash: &FileHash) -> Result<Self> {
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

        let mut s = Self {
            members,
            state_member,
            total_size,
            states,
        };

        s.sort_members_by_offset();

        Ok(s)
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

impl AsyncFnType {}

#[derive(Debug)]
pub(crate) struct MemberValue {
    pub(crate) member: Member,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct StateValue {
    pub(crate) state: State,

    pub(crate) members: Vec<MemberValue>,
    pub(crate) awaitee: Option<Box<FutureValue>>,
}

impl StateValue {
    fn new(
        state: &State,
        bytes: &[u8],
        async_fn_type: &AsyncFnType,
        future_types: &HashMap<Type, FutureType>,
    ) -> Self {
        let mut members = Vec::new();

        for member in &state.active_members {
            let member = &async_fn_type.members[*member];

            let bytes = bytes[member.offset as usize..][..member.size as usize].to_vec();

            members.push(MemberValue {
                member: member.clone(),
                bytes,
            });
        }

        let awaitee = state.awaitee.as_ref().map(|awaitee| {
            let bytes = &bytes[awaitee.offset as usize..][..awaitee.size as usize];

            let future_value = FutureValue::new(&awaitee.ty, bytes, future_types);

            Box::new(future_value)
        });

        Self {
            state: state.clone(),
            members,
            awaitee,
        }
    }
}

#[derive(Debug)]
pub(crate) struct AsyncFnValue {
    pub(crate) ty: AsyncFnType,

    /// Err value is the found discriminat value that does not have a coresponding State
    pub(crate) state_value: std::result::Result<StateValue, u64>,
}

impl AsyncFnValue {
    pub(crate) fn new(
        async_fn_type: &AsyncFnType,
        bytes: &[u8],
        future_types: &HashMap<Type, FutureType>,
    ) -> Self {
        let state_discriminant = {
            let bytes = &bytes[async_fn_type.state_member.offset as usize..];
            match async_fn_type.state_member.size {
                1 => u8::from_le_bytes(bytes[..1].try_into().unwrap()) as u64,
                2 => u16::from_le_bytes(bytes[..2].try_into().unwrap()) as u64,
                4 => u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64,
                8 => u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                _ => unreachable!(),
            }
        };

        let state = async_fn_type
            .states
            .iter()
            .find(|s| s.discriminant_value == state_discriminant);

        let state_value = state
            .map(|s| StateValue::new(s, bytes, async_fn_type, future_types))
            .ok_or(state_discriminant);

        Self {
            ty: async_fn_type.clone(),
            state_value,
        }
    }
}
