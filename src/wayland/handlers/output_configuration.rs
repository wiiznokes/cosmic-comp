// SPDX-License-Identifier: GPL-3.0-only

use smithay::output::Output;
use tracing::{error, warn};

use crate::{
    config::{OutputConfig, OutputState},
    state::State,
    wayland::protocols::output_configuration::{
        delegate_output_configuration, ModeConfiguration, OutputConfiguration,
        OutputConfigurationHandler, OutputConfigurationState,
    },
};

use std::cell::RefCell;

impl OutputConfigurationHandler for State {
    fn output_configuration_state(&mut self) -> &mut OutputConfigurationState<Self> {
        &mut self.common.output_configuration_state
    }

    fn test_configuration(&mut self, conf: Vec<(Output, OutputConfiguration)>) -> bool {
        self.output_configuration(true, conf)
    }
    fn apply_configuration(&mut self, conf: Vec<(Output, OutputConfiguration)>) -> bool {
        self.output_configuration(false, conf)
    }
}

impl State {
    fn output_configuration(
        &mut self,
        test_only: bool,
        conf: Vec<(Output, OutputConfiguration)>,
    ) -> bool {
        if conf
            .iter()
            .all(|(_, conf)| matches!(conf, OutputConfiguration::Disabled))
        {
            return false; // we don't allow the user to accidentally disable all their outputs
        }

        let mut backups = Vec::new();
        for (output, conf) in &conf {
            {
                let mut current_config = output
                    .user_data()
                    .get::<RefCell<OutputConfig>>()
                    .unwrap()
                    .borrow_mut();
                backups.push((output, current_config.clone()));

                if let OutputConfiguration::Enabled {
                    mirroring,
                    mode,
                    scale,
                    transform,
                    position,
                    adaptive_sync,
                } = conf
                {
                    match mode {
                        Some(ModeConfiguration::Mode(mode)) => {
                            current_config.mode =
                                ((mode.size.w, mode.size.h), Some(mode.refresh as u32));
                        }
                        Some(ModeConfiguration::Custom { size, refresh }) => {
                            current_config.mode = ((size.w, size.h), refresh.map(|x| x as u32));
                        }
                        _ => {}
                    }
                    if let Some(scale) = scale {
                        current_config.scale = *scale;
                    }
                    if let Some(transform) = transform {
                        current_config.transform = *transform;
                    }
                    if let Some(position) = position {
                        current_config.position = (*position).into();
                    }
                    if let Some(vrr) = adaptive_sync {
                        current_config.vrr = *vrr;
                    }
                    if let Some(mirror) = mirroring {
                        current_config.enabled = OutputState::Mirroring(mirror.name());
                    } else {
                        current_config.enabled = OutputState::Enabled;
                    }
                } else {
                    current_config.enabled = OutputState::Disabled;
                }
            }
        }

        let res = self.backend.apply_config_for_outputs(
            test_only,
            &self.common.event_loop_handle,
            self.common.shell.clone(),
            &mut self.common.workspace_state.update(),
            &self.common.xdg_activation_state,
            self.common.startup_done.clone(),
        );
        if let Err(err) = res {
            warn!(?err, "Failed to apply config. Resetting");
            for (output, backup) in backups {
                {
                    let mut current_config = output
                        .user_data()
                        .get::<RefCell<OutputConfig>>()
                        .unwrap()
                        .borrow_mut();
                    *current_config = backup;
                }
            }
            if !test_only {
                if let Err(err) = self.backend.apply_config_for_outputs(
                    false,
                    &self.common.event_loop_handle,
                    self.common.shell.clone(),
                    &mut self.common.workspace_state.update(),
                    &self.common.xdg_activation_state,
                    self.common.startup_done.clone(),
                ) {
                    error!(?err, "Failed to reset output config.");
                }
            }
            return false;
        }
        self.common.refresh();

        for output in conf
            .iter()
            .filter(|(_, c)| {
                matches!(
                    c,
                    OutputConfiguration::Enabled {
                        mirroring: None,
                        ..
                    }
                )
            })
            .map(|(o, _)| o)
        {
            self.common.output_configuration_state.enable_head(output);
        }
        for output in conf
            .iter()
            .filter(|(_, c)| {
                matches!(
                    c,
                    OutputConfiguration::Disabled
                        | OutputConfiguration::Enabled {
                            mirroring: Some(_),
                            ..
                        }
                )
            })
            .map(|(o, _)| o)
        {
            self.common.output_configuration_state.disable_head(output);
        }
        self.common
            .config
            .write_outputs(self.common.output_configuration_state.outputs());
        self.common.event_loop_handle.insert_idle(move |state| {
            state.common.output_configuration_state.update();
        });

        true
    }
}

delegate_output_configuration!(State);
