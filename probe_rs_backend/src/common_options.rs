//! Lovingly stolen from the main probe-rs cli implementation here:
//! <https://github.com/probe-rs/probe-rs/blob/master/probe-rs-tools/src/bin/probe-rs/util/common_options.rs>

use std::{io::Write, path::PathBuf};

use probe_rs::{
    Permissions, Session,
    config::{Registry, RegistryError, TargetSelector},
    integration::FakeProbe,
    probe::{
        DebugProbeError, DebugProbeInfo, DebugProbeSelector, Probe, WireProtocol, list::Lister,
    },
};

/// Common options and logic when interfacing with a [Probe].
#[derive(clap::Parser, Clone, Debug)]
pub struct ProbeOptions {
    #[arg(long, env = "PROBE_RS_CHIP", help_heading = "PROBE CONFIGURATION")]
    pub chip: Option<String>,
    #[arg(
        value_name = "chip description file path",
        long,
        env = "PROBE_RS_CHIP_DESCRIPTION_PATH",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub chip_description_path: Option<PathBuf>,

    /// Protocol used to connect to chip. Possible options: [swd, jtag]
    #[arg(long, env = "PROBE_RS_PROTOCOL", help_heading = "PROBE CONFIGURATION")]
    pub protocol: Option<WireProtocol>,

    /// Disable interactive probe selection
    #[arg(
        long,
        env = "PROBE_RS_NON_INTERACTIVE",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub non_interactive: bool,

    /// Use this flag to select a specific probe in the list.
    ///
    /// Use '--probe VID:PID' or '--probe VID:PID:Serial' if you have more than one
    /// probe with the same VID:PID.",
    #[arg(long, env = "PROBE_RS_PROBE", help_heading = "PROBE CONFIGURATION")]
    pub probe: Option<DebugProbeSelector>,
    /// The protocol speed in kHz.
    #[arg(long, env = "PROBE_RS_SPEED", help_heading = "PROBE CONFIGURATION")]
    pub speed: Option<u32>,
    /// Use this flag to assert the nreset & ntrst pins during attaching the probe to
    /// the chip.
    #[arg(
        long,
        env = "PROBE_RS_CONNECT_UNDER_RESET",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub connect_under_reset: bool,
    /// This option is not used and is only here for compatibility with probe-rs.
    #[arg(long, env = "PROBE_RS_DRY_RUN", help_heading = "PROBE CONFIGURATION")]
    pub dry_run: bool,
    /// This option is not used and is only here for compatibility with probe-rs.
    #[arg(
        long,
        env = "PROBE_RS_ALLOW_ERASE_ALL",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub allow_erase_all: bool,
}

impl ProbeOptions {
    pub fn load(self, registry: &mut Registry) -> Result<LoadedProbeOptions<'_>, OperationError> {
        LoadedProbeOptions::new(self, registry)
    }

    /// Convenience method that attaches to the specified probe, target,
    /// and target session.
    pub fn simple_attach<'r>(
        self,
        registry: &'r mut Registry,
        lister: &Lister,
    ) -> Result<(Session, LoadedProbeOptions<'r>), OperationError> {
        let common_options = self.load(registry)?;

        let target = common_options.get_target_selector()?;
        let probe = common_options.attach_probe(lister)?;
        let session = common_options.attach_session(probe, target)?;

        Ok((session, common_options))
    }
}

/// Common options and logic when interfacing with a [Probe] which already did all pre operation preparation.
pub struct LoadedProbeOptions<'r>(ProbeOptions, &'r mut Registry);

impl<'r> LoadedProbeOptions<'r> {
    /// Performs necessary init calls such as loading all chip descriptions
    /// and returns a newtype that ensures initialization.
    pub(crate) fn new(
        probe_options: ProbeOptions,
        registry: &'r mut Registry,
    ) -> Result<Self, OperationError> {
        let mut options = Self(probe_options, registry);
        // Load the target description, if given in the cli parameters.
        options.maybe_load_chip_desc()?;
        Ok(options)
    }

    /// Add targets contained in file given by --chip-description-path
    /// to probe-rs registry.
    ///
    /// Note: should be called before any functions in [ProbeOptions].
    fn maybe_load_chip_desc(&mut self) -> Result<(), OperationError> {
        if let Some(ref cdp) = self.0.chip_description_path {
            let yaml = std::fs::read_to_string(cdp).map_err(|error| {
                OperationError::ChipDescriptionNotFound {
                    source: error,
                    path: cdp.clone(),
                }
            })?;

            self.1.add_target_family_from_yaml(&yaml).map_err(|error| {
                OperationError::FailedChipDescriptionParsing {
                    source: error,
                    path: cdp.clone(),
                }
            })?;
        }

        Ok(())
    }

    /// Resolves a resultant target selector from passed [ProbeOptions].
    pub fn get_target_selector(&self) -> Result<TargetSelector, OperationError> {
        let target = if let Some(chip_name) = &self.0.chip {
            let target = self.1.get_target_by_name(chip_name).map_err(|error| {
                OperationError::ChipNotFound {
                    source: error,
                    name: chip_name.clone(),
                }
            })?;

            TargetSelector::Specified(target)
        } else {
            TargetSelector::Auto
        };

        Ok(target)
    }

    /// Allow for a stdin selection of the given probes
    fn interactive_probe_select(
        list: &[DebugProbeInfo],
    ) -> Result<&DebugProbeInfo, OperationError> {
        println!("Available Probes:");
        for (i, probe_info) in list.iter().enumerate() {
            println!("{i}: {probe_info}");
        }

        print!("Selection: ");
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .expect("Expect input for probe selection");

        let probe_idx = input
            .trim()
            .parse::<usize>()
            .map_err(OperationError::ParseProbeIndex)?;

        list.get(probe_idx).ok_or(OperationError::NoProbesFound)
    }

    /// Selects a probe from a list of probes.
    /// If there is only one probe, it will be selected automatically.
    /// If there are multiple probes, the user will be prompted to select one unless
    /// started in non-interactive mode.
    fn select_probe(lister: &Lister, non_interactive: bool) -> Result<Probe, OperationError> {
        let list = lister.list_all();
        let selected = match list.len() {
            0 | 1 => list.first().ok_or(OperationError::NoProbesFound),
            _ if non_interactive => Err(OperationError::MultipleProbesFound { list }),
            _ => Self::interactive_probe_select(&list),
        };

        match selected {
            Ok(probe_info) => Ok(lister.open(probe_info)?),
            Err(error) => Err(error),
        }
    }

    /// Attaches to specified probe and configures it.
    pub fn attach_probe(&self, lister: &Lister) -> Result<Probe, OperationError> {
        let mut probe = if self.0.dry_run {
            Probe::from_specific_probe(Box::new(FakeProbe::with_mocked_core()))
        } else {
            // If we got a probe selector as an argument, open the probe
            // matching the selector if possible.
            match &self.0.probe {
                Some(selector) => lister.open(selector)?,
                None => Self::select_probe(lister, self.0.non_interactive)?,
            }
        };

        if let Some(protocol) = self.0.protocol {
            // Select protocol and speed
            probe.select_protocol(protocol).map_err(|error| {
                OperationError::FailedToSelectProtocol {
                    source: error,
                    protocol,
                }
            })?;
        }

        if let Some(speed) = self.0.speed {
            let _actual_speed = probe.set_speed(speed).map_err(|error| {
                OperationError::FailedToSelectProtocolSpeed {
                    source: error,
                    speed,
                }
            })?;

            // Warn the user if they specified a speed the debug probe does not support
            // and a fitting speed was automatically selected.
            let protocol_speed = probe.speed_khz();
            if let Some(speed) = self.0.speed {
                if protocol_speed < speed {
                    log::warn!(
                        "Unable to use specified speed of {} kHz, actual speed used is {} kHz",
                        speed,
                        protocol_speed
                    );
                }
            }

            log::info!("Protocol speed {} kHz", protocol_speed);
        }

        Ok(probe)
    }

    /// Attaches to target device session. Attaches under reset if
    /// specified by [ProbeOptions::connect_under_reset].
    pub fn attach_session(
        &self,
        probe: Probe,
        target: TargetSelector,
    ) -> Result<Session, OperationError> {
        let mut permissions = Permissions::new();
        if self.0.allow_erase_all {
            permissions = permissions.allow_erase_all();
        }

        let session = if self.0.connect_under_reset {
            probe.attach_under_reset(target, permissions)
        } else {
            probe.attach(target, permissions)
        }
        .map_err(|error| OperationError::AttachingFailed {
            source: error,
            connect_under_reset: self.0.connect_under_reset,
        })?;

        Ok(session)
    }
}

impl AsRef<ProbeOptions> for LoadedProbeOptions<'_> {
    fn as_ref(&self) -> &ProbeOptions {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    #[error("No connected probes were found.")]
    NoProbesFound,

    #[error("Failed to open the debug probe.")]
    FailedToOpenProbe(#[from] DebugProbeError),

    #[error("{} probes were found: {}", .list.len(), print_list(.list))]
    MultipleProbesFound { list: Vec<DebugProbeInfo> },

    #[error("Failed to open the chip description '{path}'.")]
    ChipDescriptionNotFound {
        source: std::io::Error,
        path: PathBuf,
    },

    #[error("Failed to parse the chip description '{path}'.")]
    FailedChipDescriptionParsing {
        source: RegistryError,
        path: PathBuf,
    },

    #[error("The chip '{name}' was not found in the database.")]
    ChipNotFound { source: RegistryError, name: String },

    #[error("The protocol '{protocol}' could not be selected.")]
    FailedToSelectProtocol {
        source: DebugProbeError,
        protocol: WireProtocol,
    },

    #[error("The protocol speed could not be set to '{speed}' kHz.")]
    FailedToSelectProtocolSpeed { source: DebugProbeError, speed: u32 },

    #[error("Connecting to the chip was unsuccessful.")]
    AttachingFailed {
        source: probe_rs::Error,
        connect_under_reset: bool,
    },
    #[error("Failed to write to file")]
    IOError(#[source] std::io::Error),

    #[error("Failed to parse CLI arguments.")]
    CliArgument(#[from] clap::Error),
    #[error("Failed to parse interactive probe index selection")]
    ParseProbeIndex(#[source] std::num::ParseIntError),
}

/// Used in errors it can print a list of items.
fn print_list(list: &[impl std::fmt::Display]) -> String {
    let mut output = String::new();

    for (i, entry) in list.iter().enumerate() {
        output.push_str(&format!("\n    {}. {}", i + 1, entry));
    }

    output
}

impl From<std::io::Error> for OperationError {
    fn from(e: std::io::Error) -> Self {
        OperationError::IOError(e)
    }
}
