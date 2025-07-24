use std::{path::Path, sync::OnceLock};

use dwarf_reader::{FutureType, Layout, Member};

use crate::debug_reader::{DebugData, normalize_name};

mod debug_reader;

static DEBUG_DATA: OnceLock<DebugData> = OnceLock::new();

/// Initilize the inspector using the debug data from the given path.
pub fn initialize_from_path<P: AsRef<Path>>(path: P) {
    let debug_data = DebugData::from_path(path);

    eprintln!("Initializing debug data:");
    for layout in debug_data.future_types.values() {
        eprintln!("{layout}");
    }

    if DEBUG_DATA.set(debug_data).is_err() {
        eprintln!("Already initialized");
    }
}

/// Initilize the inspector using the debug data from the running executable
pub fn initialize() {
    let path = process_path::get_executable_path().unwrap();
    initialize_from_path(path);
}

struct Inspector<F: Future> {
    inner: F,

    name: String,
    future_type: Option<FutureType>,
}

impl<F: Future> Future for Inspector<F> {
    type Output = F::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let name = self.name.clone();

        // SAFETY: inner is a field so is pined if self is. This is also the only place inner is
        // accessed so it can't be moved out.
        let mut inner = unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.inner) };

        let result = Future::poll(inner.as_mut(), cx);
        match &result {
            std::task::Poll::Ready(_) => {
                eprintln!("Future {} got polled and returned: Ready", name)
            }
            std::task::Poll::Pending => {
                eprintln!("Future {} got polled and returned: Pending", name)
            }
        }

        let bytes = (&*inner) as *const F as *const u8;

        let future_type = unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.future_type) };
        if let Some(future_type) = &*future_type {
            assert_eq!(
                future_type.layout.total_size,
                std::mem::size_of::<F>() as u64
            );
            // This is only safe if the bytes actually match the layout, this can not be garunteed
            // realy, to bad...
            unsafe {
                describe_future(&future_type.layout, bytes, "");
            }
        }

        eprintln!("");

        result
    }
}

unsafe fn print_bytes(bytes: *const u8, length: u64) {
    for i in 0..length {
        let b = unsafe { *bytes.offset(i as isize) };
        eprint!("{b:02x} ")
    }
}

unsafe fn describe_member(member: &Member, bytes: *const u8, padding: &str) {
    eprint!("{padding} - {}: {} = ", member.name, member.type_name);
    unsafe {
        let bytes = bytes.offset(member.offset as isize);
        match member.type_name.as_str() {
            "bool" => eprintln!("{:?}", std::mem::transmute::<_, &bool>(bytes)),
            "u8" => eprintln!("{:?}", std::mem::transmute::<_, &u8>(bytes)),
            "i8" => eprintln!("{:?}", std::mem::transmute::<_, &i8>(bytes)),
            "u16" => eprintln!("{:?}", std::mem::transmute::<_, &u16>(bytes)),
            "i16" => eprintln!("{:?}", std::mem::transmute::<_, &i16>(bytes)),
            "u32" => eprintln!("{:?}", std::mem::transmute::<_, &u32>(bytes)),
            "i32" => eprintln!("{:?}", std::mem::transmute::<_, &i32>(bytes)),
            "u64" => eprintln!("{:?}", std::mem::transmute::<_, &u64>(bytes)),
            "i64" => eprintln!("{:?}", std::mem::transmute::<_, &i64>(bytes)),
            "u128" => eprintln!("{:?}", std::mem::transmute::<_, &u128>(bytes)),
            "i128" => eprintln!("{:?}", std::mem::transmute::<_, &i128>(bytes)),
            "f32" => eprintln!("{:?}", std::mem::transmute::<_, &f32>(bytes)),
            "f64" => eprintln!("{:?}", std::mem::transmute::<_, &f64>(bytes)),
            "alloc::string::String" => {
                eprintln!("{:?}", std::mem::transmute::<_, &String>(bytes))
            }
            _ => {
                eprint!("bytes = ",);
                print_bytes(bytes, member.size);
                eprintln!("");
            }
        }
    }
}

unsafe fn describe_future(layout: &Layout, bytes: *const u8, padding: &str) {
    let state_member = &layout.state_member;
    let state_discriminant = unsafe {
        let bytes = bytes.offset(state_member.offset as isize);
        match state_member.size {
            1 => std::ptr::read_unaligned(bytes as *const u8) as u64,
            2 => std::ptr::read_unaligned(bytes as *const u16) as u64,
            4 => std::ptr::read_unaligned(bytes as *const u32) as u64,
            8 => std::ptr::read_unaligned(bytes as *const u64),
            _ => unreachable!(),
        }
    };

    let state = layout
        .states
        .iter()
        .find(|s| s.discriminant_value == state_discriminant);
    let Some(state) = state else {
        let valid_states = layout
            .states
            .iter()
            .map(|s| s.discriminant_value)
            .collect::<Vec<_>>();
        eprintln!(
            "Future has invalid state discriminant: {state_discriminant}. Valid states are: {:?}",
            valid_states
        );
        return;
    };

    eprintln!(
        "{padding}it has a discriminant of {} corrosponding to a state of {} and has the members:",
        state_discriminant, state.name
    );

    for member in &state.active_members {
        let member = &layout.members[*member];
        unsafe {
            describe_member(member, bytes, padding);
        }
    }

    let Some(awaitee) = &state.awaitee else {
        return;
    };

    eprintln!(
        "{padding}and is waiting on the interior Future {}:",
        awaitee.type_name
    );

    let normalized_name = normalize_name(&awaitee.type_name);
    let future_type = DEBUG_DATA
        .get()
        .and_then(|dd| dd.future_types.get(&normalized_name).cloned());

    let bytes = unsafe { bytes.offset(awaitee.offset as isize) };
    match future_type {
        Some(future_type) => unsafe {
            let s = format!("{padding}  ");
            describe_future(&future_type.layout, bytes, &s);
        },
        None => {
            eprint!("{padding}  bytes = ",);
            unsafe { print_bytes(bytes, awaitee.size) };
            eprintln!("");
        }
    }
}

pub fn inspect<F: IntoFuture>(f: F) -> impl Future<Output = F::Output> {
    let future = f.into_future();
    let name = std::any::type_name_of_val(&future);
    let normalized_name = normalize_name(name);

    let future_type = DEBUG_DATA
        .get()
        .and_then(|dd| dd.future_types.get(&normalized_name).cloned());

    Inspector {
        name: name.to_owned(),
        inner: future,
        future_type,
    }
}
