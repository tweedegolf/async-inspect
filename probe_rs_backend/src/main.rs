use std::{path::PathBuf, time::Duration};

use anyhow::Result;
use clap::Parser;
use probe_rs::{
    Core, MemoryInterface,
    architecture::{
        arm::ArmError, riscv::communication_interface::RiscvError,
        xtensa::communication_interface::XtensaError,
    },
    config::Registry,
    probe::{DebugProbeError, list::Lister},
};

use embassy_inspect::{Callback, Click, Event};

use common_options::ProbeOptions;
use ratatui::{
    crossterm::{
        ExecutableCommand as _,
        event::{self, MouseEventKind},
        terminal::{disable_raw_mode, enable_raw_mode},
    },
    prelude::CrosstermBackend,
};

mod common_options;

#[derive(clap::Parser, Debug)]
#[clap(
    name = "probe-rs-backend",
    about = "The probe-rs backend for embassy inspect"
)]
struct Cli {
    /// The path to the ELF file that has been flashed on the chip.
    #[clap(index = 1)]
    pub(crate) path: PathBuf,

    #[clap(flatten)]
    common: ProbeOptions,

    #[clap(long, default_value = "0")]
    core: usize,
}

fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        hook(info);
    }));
}

fn init() -> Result<impl ratatui::backend::Backend> {
    let mut stdout = std::io::stdout();
    enable_raw_mode()?;
    stdout.execute(ratatui::crossterm::terminal::EnterAlternateScreen)?;
    stdout.execute(event::EnableMouseCapture)?;

    Ok(CrosstermBackend::new(std::io::stdout()))
}

fn restore() -> Result<()> {
    let mut stdout = std::io::stdout();
    stdout.execute(event::DisableMouseCapture)?;
    disable_raw_mode()?;
    stdout.execute(ratatui::crossterm::terminal::LeaveAlternateScreen)?;
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut registry = Registry::from_builtin_families();
    let lister = Lister::new();

    let (mut session, _options) = cli.common.simple_attach(&mut registry, &lister)?;
    let core = session.core(cli.core)?;

    set_panic_hook();
    let backend = init()?;

    // TODO: Should not be a string, problem is that ddbug also takes a String
    let result = run(backend, core, &[cli.path.to_string_lossy().into_owned()]);

    ratatui::restore();

    result
}

/// Return Err on a error and Ok(None) when there are no events
fn poll_event() -> Result<Option<Event>> {
    if !event::poll(Duration::default())? {
        return Ok(None);
    }

    let event = match event::read()? {
        event::Event::Key(key_event) => {
            if key_event.modifiers.contains(event::KeyModifiers::CONTROL)
                && key_event.code == event::KeyCode::Char('c')
            {
                anyhow::bail!("Ctrl+C pressed");
            }
            return Ok(None);
        }
        event::Event::Mouse(mouse_event) => match mouse_event.kind {
            MouseEventKind::Down(button) => {
                let button = match button {
                    event::MouseButton::Left => embassy_inspect::ClickButton::Left,
                    event::MouseButton::Right => embassy_inspect::ClickButton::Right,
                    event::MouseButton::Middle => embassy_inspect::ClickButton::Middle,
                };
                Event::Click(Click {
                    pos: ratatui::layout::Position {
                        x: mouse_event.column,
                        y: mouse_event.row,
                    },
                    button,
                })
            }
            MouseEventKind::ScrollDown => Event::Scroll(-3),
            MouseEventKind::ScrollUp => Event::Scroll(3),
            _ => {
                return Ok(None);
            }
        },
        event::Event::Resize(_, _) => Event::Redraw,
        _ => {
            return Ok(None);
        }
    };

    Ok(Some(event))
}

fn run<B: ratatui::backend::Backend>(
    backend: B,
    mut core: Core,
    object_files: &[String],
) -> Result<()> {
    let mut callback = ProbeRsCallback {
        core: &mut core,
        object_files,
    };

    let mut embassy_inspector = embassy_inspect::EmbassyInspector::new(backend, &mut callback)?;

    loop {
        if let Some(event) = poll_event()? {
            embassy_inspector.handle_event(event, &mut callback)?;
            continue;
        }

        // 10 ms was the highest value where I still felt it was responsive
        match callback
            .core
            .wait_for_core_halted(Duration::from_millis(10))
        {
            Ok(()) => {
                let addr = callback
                    .core
                    .read_core_reg(callback.core.program_counter())?;
                embassy_inspector.handle_event(Event::Breakpoint(addr), &mut callback)?;
            }
            Err(
                probe_rs::Error::Timeout
                | probe_rs::Error::Probe(DebugProbeError::Timeout)
                | probe_rs::Error::Arm(ArmError::Timeout | ArmError::Probe(DebugProbeError::Timeout))
                | probe_rs::Error::Riscv(
                    RiscvError::Timeout | RiscvError::DebugProbe(DebugProbeError::Timeout),
                )
                | probe_rs::Error::Xtensa(
                    XtensaError::Timeout | XtensaError::DebugProbe(DebugProbeError::Timeout),
                ),
            ) => {}
            Err(other_err) => Err(other_err)?,
        }
    }
}

struct ProbeRsCallback<'a, 'probe> {
    core: &'a mut Core<'probe>,
    object_files: &'a [String],
}

impl<'a, 'b> Callback for ProbeRsCallback<'a, 'b> {
    fn get_objectfiles(&mut self) -> Result<impl Iterator<Item = String>> {
        Ok(self.object_files.into_iter().cloned())
    }

    fn set_breakpoint(&mut self, addr: u64) -> Result<u64> {
        self.core.set_hw_breakpoint(addr)?;
        Ok(addr)
    }

    fn resume(&mut self) -> Result<()> {
        self.core.run()?;
        Ok(())
    }

    fn read_memory(&mut self, addr: u64, len: u64) -> Result<Vec<u8>> {
        let mut buf = vec![0; len.next_multiple_of(4) as usize];
        self.core.read_mem_32bit(addr, &mut buf)?;

        buf.truncate(len as usize);

        Ok(buf)
    }

    fn try_format_value(
        &mut self,
        _bytes: &[u8],
        _ty: &embassy_inspect::ty::Type,
    ) -> Option<String> {
        None
    }
}
