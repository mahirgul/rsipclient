//! rsipclient — multi-account SIP client, RTP media engine and IVR.
//!
//! The binary (`src/main.rs`) is a thin CLI on top of this library; exposing
//! the modules as a library lets integration tests and fuzz targets drive the
//! SIP, SDP, RTP and WAV code directly.
#![allow(clippy::too_many_arguments)]

pub mod cli;
pub mod config;
pub mod ipc;
pub mod ipc_client;
pub mod ivr;
pub mod plugins;
pub mod rtp;
pub mod service;
pub mod sip;
pub mod win32_gui;
