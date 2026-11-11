//! Blueprint campaigns (FR-SP-7, `docs/tech/sim-program.md` §4.4). Filled in below.

use storage::DataRoot;

use crate::CliResult;
use crate::campaign::CampaignFile;

pub fn campaign(c: &CampaignFile, _root: &DataRoot) -> CliResult {
    Err(format!("{}: blueprint campaigns are not built yet", c.name))
}
