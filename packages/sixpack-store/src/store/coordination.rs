use super::*;

impl LocalStore {
    pub(super) fn workspace_lock_file(&self) -> io::Result<File> {
        self.ensure_workspace_layout()?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.workspace_lock_path())
    }

    pub(super) fn workspace_read_guard(&self) -> io::Result<WorkspaceReadGuard<'_>> {
        let process = self
            .workspace_gate
            .read()
            .map_err(|_| io::Error::other("workspace gate lock poisoned"))?;
        let file = self.workspace_lock_file()?;
        file.lock_shared()?;
        Ok(WorkspaceReadGuard {
            _process: process,
            _file: file,
        })
    }

    pub(super) fn workspace_write_guard(&self) -> io::Result<WorkspaceWriteGuard<'_>> {
        let process = self
            .workspace_gate
            .write()
            .map_err(|_| io::Error::other("workspace gate lock poisoned"))?;
        let file = self.workspace_lock_file()?;
        file.lock()?;
        Ok(WorkspaceWriteGuard {
            _process: process,
            _file: file,
        })
    }

    pub(super) fn current_revision(&self) -> io::Result<u64> {
        let path = self.revision_path();
        if !path.exists() {
            return Ok(0);
        }
        let value = fs::read_to_string(path)?;
        value.trim().parse::<u64>().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad workspace revision: {error}"),
            )
        })
    }

    pub(super) fn refresh_if_revision_changed(&self) -> io::Result<()> {
        let revision = self.current_revision()?;
        let observed = *self
            .observed_revision
            .read()
            .map_err(|_| io::Error::other("observed revision lock poisoned"))?;
        if observed == Some(revision) {
            return Ok(());
        }

        if observed.is_some() {
            self.invalidate_cached_state()?;
        }

        *self
            .observed_revision
            .write()
            .map_err(|_| io::Error::other("observed revision lock poisoned"))? = Some(revision);
        Ok(())
    }

    pub(super) fn invalidate_cached_state(&self) -> io::Result<()> {
        self.sixb_cache
            .write()
            .map_err(|_| io::Error::other("sixb cache lock poisoned"))?
            .clear();
        self.runtime_sixb_cache
            .write()
            .map_err(|_| io::Error::other("runtime sixb cache lock poisoned"))?
            .clear();
        self.row_cache
            .write()
            .map_err(|_| io::Error::other("row cache lock poisoned"))?
            .clear();
        self.chunk_cache
            .write()
            .map_err(|_| io::Error::other("chunk cache lock poisoned"))?
            .clear();
        self.chunk_len_cache
            .write()
            .map_err(|_| io::Error::other("chunk length cache lock poisoned"))?
            .clear();
        *self
            .next_tx_cache
            .write()
            .map_err(|_| io::Error::other("next tx cache lock poisoned"))? = None;
        self.next_chunk_cache
            .write()
            .map_err(|_| io::Error::other("next chunk cache lock poisoned"))?
            .clear();
        Ok(())
    }

    pub(super) fn activate_schema(&self, schema: &DatabaseSchema) -> io::Result<()> {
        let schema_hash = schema.schema_hash();
        let mut active = self
            .active_schema_hash
            .write()
            .map_err(|_| io::Error::other("active schema lock poisoned"))?;
        if active.as_deref() == Some(schema_hash.as_str()) {
            return Ok(());
        }
        self.invalidate_cached_state()?;
        *active = Some(schema_hash);
        Ok(())
    }

    pub(super) fn publish_revision(&self, revision: u64) -> io::Result<()> {
        let path = self.revision_path();
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        writeln!(file, "{revision}")?;
        file.sync_data()?;
        fs::rename(tmp, path)?;
        *self
            .observed_revision
            .write()
            .map_err(|_| io::Error::other("observed revision lock poisoned"))? = Some(revision);
        Ok(())
    }
}
