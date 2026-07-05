// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use calloop::channel::*;
use cctk::{
    sctk::{
        self,
        output::{OutputHandler, OutputState},
        reexports::{
            calloop,
            calloop_wayland_source::WaylandSource,
            client::{self as wayland_client},
            protocols::ext::{
                foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
                workspace::v1::client::ext_workspace_handle_v1::ExtWorkspaceHandleV1,
            },
        },
        registry::{ProvidesRegistryState, RegistryState},
    },
    toplevel_info::{ToplevelInfo, ToplevelInfoHandler, ToplevelInfoState},
    workspace::{Workspace, WorkspaceHandler, WorkspaceState},
};
use futures::{SinkExt, channel::mpsc, executor::block_on};
use std::os::{
    fd::{FromRawFd, RawFd},
    unix::net::UnixStream,
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::wl_output::{self, WlOutput},
};

#[derive(Debug, Clone)]
pub enum WorkspaceEvent {
    Activate(ExtWorkspaceHandleV1),
}

#[derive(Debug, Clone)]
pub struct WorkspaceSnapshot {
    pub workspaces: Vec<Workspace>,
    pub toplevels: Vec<ToplevelInfo>,
    pub output: Option<WlOutput>,
}

pub fn spawn_workspaces(tx: mpsc::Sender<WorkspaceSnapshot>) -> SyncSender<WorkspaceEvent> {
    let (workspaces_tx, workspaces_rx) = calloop::channel::sync_channel(100);

    let socket = std::env::var("X_PRIVILEGED_WAYLAND_SOCKET")
        .ok()
        .and_then(|fd| {
            fd.parse::<RawFd>()
                .ok()
                .map(|fd| unsafe { UnixStream::from_raw_fd(fd) })
        });

    let conn = if let Some(socket) = socket {
        Connection::from_socket(socket)
    } else {
        Connection::connect_to_env()
    }
    .map_err(anyhow::Error::msg);

    if let Ok(conn) = conn {
        std::thread::spawn(move || {
            let configured_output = std::env::var("COSMIC_PANEL_OUTPUT")
                .ok()
                .unwrap_or_default();
            let mut event_loop = calloop::EventLoop::<State>::try_new().unwrap();
            let loop_handle = event_loop.handle();
            let (globals, event_queue) = registry_queue_init(&conn).unwrap();
            let qhandle = event_queue.handle();

            WaylandSource::new(conn, event_queue)
                .insert(loop_handle)
                .unwrap();

            let registry_state = RegistryState::new(&globals);
            let mut state = State {
                // Must be before `WorkspaceState`
                output_state: OutputState::new(&globals, &qhandle),
                configured_output,
                workspace_state: WorkspaceState::new(&registry_state, &qhandle),
                toplevel_info_state: ToplevelInfoState::new(&registry_state, &qhandle),
                registry_state,
                expected_output: None,
                tx,
                running: true,
                have_workspaces: false,
            };
            let loop_handle = event_loop.handle();
            loop_handle
                .insert_source(workspaces_rx, |e, (), state| match e {
                    Event::Msg(WorkspaceEvent::Activate(handle)) => {
                        handle.activate();
                        state
                            .workspace_state
                            .workspace_manager()
                            .get()
                            .unwrap()
                            .commit();
                    }
                    Event::Closed => {
                        if let Ok(workspace_manager) =
                            state.workspace_state.workspace_manager().get()
                        {
                            for g in state.workspace_state.workspace_groups() {
                                g.handle.destroy();
                            }
                            workspace_manager.stop();
                        }
                    }
                })
                .unwrap();
            while state.running {
                event_loop.dispatch(None, &mut state).unwrap();
            }
        });
    } else {
        eprintln!("ENV variable WAYLAND_DISPLAY is missing. Exiting...");
        std::process::exit(1);
    }

    workspaces_tx
}

#[derive(Debug)]
pub struct State {
    running: bool,
    tx: mpsc::Sender<WorkspaceSnapshot>,
    configured_output: String,
    expected_output: Option<WlOutput>,
    output_state: OutputState,
    registry_state: RegistryState,
    workspace_state: WorkspaceState,
    toplevel_info_state: ToplevelInfoState,
    have_workspaces: bool,
}

impl State {
    pub fn workspace_list(&self) -> Vec<Workspace> {
        self.workspace_state
            .workspace_groups()
            .filter(|g| {
                g.outputs
                    .iter()
                    .any(|o| Some(o) == self.expected_output.as_ref())
            })
            .flat_map(|g| {
                g.workspaces
                    .iter()
                    .filter_map(|handle| self.workspace_state.workspace_info(handle))
            })
            .cloned()
            .collect()
    }

    fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            workspaces: self.workspace_list(),
            toplevels: self.toplevel_info_state.toplevels().cloned().collect(),
            output: self.expected_output.clone(),
        }
    }

    fn send_snapshot(&mut self) {
        let snapshot = self.snapshot();
        let _ = block_on(self.tx.send(snapshot));
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    sctk::registry_handlers![OutputState,];
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let info = self.output_state.info(&output).unwrap();
        if info.name.as_deref() == Some(&self.configured_output) {
            self.expected_output = Some(output);
            if self.have_workspaces {
                self.send_snapshot();
            }
        }
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.expected_output.as_ref() == Some(&output) {
            self.expected_output = None;
            self.send_snapshot();
        }
    }
}

impl WorkspaceHandler for State {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspace_state
    }

    fn done(&mut self) {
        self.have_workspaces = true;
        self.send_snapshot();
    }
}

impl ToplevelInfoHandler for State {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevel_info_state
    }

    fn new_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.send_snapshot();
    }

    fn update_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _toplevel: &ExtForeignToplevelHandleV1,
    ) {
        self.send_snapshot();
    }

    fn toplevel_closed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        toplevel: &ExtForeignToplevelHandleV1,
    ) {
        // The toolkit invokes this callback immediately before removing the
        // closed toplevel from its state, so omit it from this snapshot.
        let mut snapshot = self.snapshot();
        snapshot
            .toplevels
            .retain(|info| info.foreign_toplevel != *toplevel);
        let _ = block_on(self.tx.send(snapshot));
    }
}

cctk::delegate_toplevel_info!(State);
cctk::delegate_workspace!(State);
sctk::delegate_output!(State);
sctk::delegate_registry!(State);
