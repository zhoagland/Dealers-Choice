//! The data directory layout (FR-ST-2…4, `docs/tech/storage.md` §1):
//!
//! ```text
//! <data_root>/                        default ./data, or $DC_DATA_ROOT, or --data-root
//! ├── dc.sqlite
//! ├── sim/game=<id>/run=<run_id>/
//! │   ├── spec.toml   summary.json   hands-000.bin.zst …
//! └── blueprints/<game_id>/v<version>-<iterations>.bin
//! ```
//!
//! `key=value` directory names ("hive partitioning") let DuckDB query across runs directly,
//! e.g. `read_json('data/sim/*/*/summary.json', hive_partitioning = true)`.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use sim::{RecordSink, RunReport, RunSpec};

/// Where everything lives.
#[derive(Clone, Debug)]
pub struct DataRoot(pub PathBuf);

impl DataRoot {
    /// `explicit` (a CLI flag) wins, then `$DC_DATA_ROOT`, then `./data`.
    pub fn resolve(explicit: Option<&Path>) -> DataRoot {
        let p = explicit
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("DC_DATA_ROOT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("data"));
        DataRoot(p)
    }

    pub fn db(&self) -> PathBuf {
        self.0.join("dc.sqlite")
    }

    pub fn blueprint(&self, game_id: &str, version: u32, iterations: u64) -> PathBuf {
        self.0
            .join("blueprints")
            .join(game_id)
            .join(format!("v{version}-{iterations}.bin"))
    }

    pub fn sim_run(&self, game_id: &str, run_id: &str) -> PathBuf {
        self.0
            .join("sim")
            .join(format!("game={game_id}"))
            .join(format!("run={run_id}"))
    }
}

/// A run id that sorts by time and records the seed: `<unix seconds>-<seed hex>`.
pub fn run_id(seed: u64) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}-{seed:016x}")
}

/// Writes kept hand records into zstd-compressed files, starting a new file every
/// `ROTATE_BYTES` of input so no single file grows without bound.
pub struct ZstdRecordSink {
    dir: PathBuf,
    file_index: usize,
    written: usize,
    encoder: Option<zstd::Encoder<'static, File>>,
}

const ROTATE_BYTES: usize = 256 * 1024 * 1024;

impl ZstdRecordSink {
    pub fn new(dir: &Path) -> std::io::Result<ZstdRecordSink> {
        std::fs::create_dir_all(dir)?;
        Ok(ZstdRecordSink {
            dir: dir.to_path_buf(),
            file_index: 0,
            written: 0,
            encoder: None,
        })
    }

    fn encoder(&mut self) -> &mut zstd::Encoder<'static, File> {
        if self.encoder.is_none() || self.written >= ROTATE_BYTES {
            self.finish_current();
            let path = self
                .dir
                .join(format!("hands-{:03}.bin.zst", self.file_index));
            self.file_index += 1;
            self.written = 0;
            let file = File::create(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            // Level 3: zstd's default, a good speed/ratio balance.
            self.encoder = Some(zstd::Encoder::new(file, 3).expect("zstd encoder"));
        }
        self.encoder.as_mut().unwrap()
    }

    fn finish_current(&mut self) {
        if let Some(enc) = self.encoder.take() {
            enc.finish().expect("finishing zstd file");
        }
    }

    /// Flush and close the last file. Also happens on drop, but this surfaces errors.
    pub fn finish(mut self) {
        self.finish_current();
    }
}

impl RecordSink for ZstdRecordSink {
    fn write(&mut self, record: &[u8]) {
        self.encoder()
            .write_all(record)
            .expect("writing hand records");
        self.written += record.len();
    }
}

impl Drop for ZstdRecordSink {
    fn drop(&mut self) {
        self.finish_current();
    }
}

/// Write `spec.toml` and `summary.json` for a finished run.
pub fn write_run_files(dir: &Path, spec: &RunSpec, report: &RunReport) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let spec_text = toml::to_string_pretty(spec).map_err(std::io::Error::other)?;
    std::fs::write(dir.join("spec.toml"), spec_text)?;
    let json = serde_json::to_string_pretty(report).map_err(std::io::Error::other)?;
    std::fs::write(dir.join("summary.json"), json)
}

/// Read every record back from a run directory (decompressing), in file order.
pub fn read_run_records(dir: &Path) -> std::io::Result<Vec<u8>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "zst"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        out.extend(zstd::decode_all(File::open(f)?)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zstd_sink_round_trips() {
        let dir = std::env::temp_dir().join(format!("dc-sink-{}", std::process::id()));
        let mut sink = ZstdRecordSink::new(&dir).unwrap();
        let records: Vec<Vec<u8>> = (0..1000u32).map(|i| i.to_le_bytes().repeat(5)).collect();
        for r in &records {
            sink.write(r);
        }
        sink.finish();
        assert_eq!(read_run_records(&dir).unwrap(), records.concat());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn layout_paths() {
        let root = DataRoot(PathBuf::from("d"));
        assert_eq!(
            root.sim_run("ftq", "1-2"),
            PathBuf::from("d/sim/game=ftq/run=1-2")
        );
        assert_eq!(
            root.blueprint("kuhn", 1, 200),
            PathBuf::from("d/blueprints/kuhn/v1-200.bin")
        );
    }
}
