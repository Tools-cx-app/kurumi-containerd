// Copyright (C) 2026 Tools-cx-app
// SPDX-License-Identifier: GPL-v3-only

use anyhow::Result;
use std::path::Path;

use crate::registry::{Entry, load_entries, refresh};

pub(super) struct Backend<'a> {
    _exe: &'a Path,
}

impl<'a> Backend<'a> {
    pub(super) fn new(exe: &'a Path) -> Self {
        Self { _exe: exe }
    }

    #[allow(clippy::unused_self)]
    pub(super) fn load_entries(&self) -> Result<Vec<Entry>> {
        load_entries()
    }

    #[allow(clippy::unused_self)]
    pub(super) fn refresh(&self, entries: &mut [Entry]) {
        refresh(entries);
    }
}
