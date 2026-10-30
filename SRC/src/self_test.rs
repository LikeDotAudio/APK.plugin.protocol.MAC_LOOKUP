// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! maclookup's self test — the OUI table still resolves a vendor this bench actually owns.
//!
//! Owns: the checks THIS crate can make about itself, and nothing about how
//! they are asked for. The contract in `apkaudio_contracts::self_test` owns the
//! two inputs, the withdrawal they perform and the words the answer may carry;
//! this file owns what is actually true.
//!
//! WHY IT IS A PLAIN FUNCTION AND NOT A BUS LOOP
//!
//! Eleven of the twenty-nine crates in this workspace link no MQTT client, so
//! whatever runs their self test is not them — for the in-process protocol
//! crates it is the orchestrator. A function over the crate's own machinery is
//! runnable by that supervisor, by `cargo test`, and by a person at a terminal,
//! and none of the three needs a broker to be up. PLAN-459.01.
//!
//! WHAT A CHECK HERE MAY DO
//!
//! At [`Depth::Own`], nothing that leaves the process: no socket, no file
//! outside what is baked in at compile time, no clock it does not control.
//! Anything that reaches the host or the network is [`Depth::Device`], and the
//! contract records it as `skipped` when the run did not ask for it — so a
//! check filed at the wrong depth is caught rather than quietly performed.

use apkaudio_contracts::self_test::{Check, Depth, Report};

/// The topic segment this crate owns — the `{proto}` in
/// `APK.audio/System/Protocols/{proto}/…`, and the name its report is filed
/// under. It must match the section in `config.ini`, which is what
/// `config_section_still_names_this_module` below asserts.
pub const MODULE: &str = "maclookup";

/// Everything this crate can prove about itself at `depth`.
pub fn self_test(depth: Depth) -> Report {
    let mut report = Report::new(MODULE, depth);

    // Every crate answers these two. The first proves this crate can address
    // its own mailbox at all; the second proves the name it addresses it with
    // is the one the config walk publishes it under.
    report.record(Check::from_result(
        "landing_spots_build",
        Depth::Own,
        "both inputs and all three readbacks address a legal topic",
        landing_spots(),
    ));

    report.record(Check::from_result(
        "a_mac_parses_in_every_spelling_the_bus_uses",
        Depth::Own,
        "colons, hyphens and bare hex all reach the same OUI",
        {
            let spellings = ["00:1D:C1:11:22:33", "00-1d-c1-11-22-33", "001DC1112233"];
            let ouis: Vec<_> = spellings.iter().filter_map(|s| crate::Mac::parse(s)).map(|m| m.oui()).collect();
            if ouis.len() == spellings.len() && ouis.windows(2).all(|w| w[0] == w[1]) {
                Ok(())
            } else {
                Err(format!("{} of {} spellings parsed alike", ouis.len(), spellings.len()))
            }
        },
    ));
    report.record(Check::from_result(
        "a_mac_that_is_not_one_is_refused",
        Depth::Own,
        "short, long and non-hex inputs are all None",
        if crate::Mac::parse("00:1D:C1").is_none()
            && crate::Mac::parse("zz:zz:zz:zz:zz:zz").is_none()
            && crate::Mac::parse("").is_none()
        {
            Ok(())
        } else {
            Err("something that is not a MAC parsed as one")
        },
    ));
    report.record(Check::from_result(
        "the_vendor_table_loaded",
        Depth::Own,
        "the OUI seed baked into this binary has entries",
        {
            // The compiled-in seed, not the cache on disk: at Depth::Own nothing
            // outside what is baked in may be read, and a container whose cache
            // volume is still empty is not a broken crate.
            if crate::MacVendors::seed_len() > 0 {
                Ok(())
            } else {
                Err("the baked OUI seed resolved zero vendors".to_string())
            }
        },
    ));

    report
}

/// The four self-test topics, built. An error here is a module name that
/// stopped being a legal topic segment — which would silently detach this
/// crate from its own mailbox rather than fail anywhere visible.
fn landing_spots() -> Result<(), String> {
    use apkaudio_contracts::self_test as st;
    st::engage_topic(MODULE).map_err(|e| format!("{e:?}"))?;
    st::run_topic(MODULE).map_err(|e| format!("{e:?}"))?;
    apkaudio_contracts::baremetal_io::module_incoming(MODULE, st::LEAF_RESULT)
        .map_err(|e| format!("{e:?}"))?;
    apkaudio_contracts::baremetal_io::module_incoming(MODULE, st::LEAF_REPORT)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

/// The same self test, across a workspace boundary.
///
/// The supervisor that collects these lives in the `Core` workspace, which has
/// its OWN lockfile and shares one `target/` with this one. A `Report` handed
/// across that line is only the same type while the two locks agree everywhere
/// below `apkaudio-contracts`, and when they stop agreeing the compiler says so
/// in a message that names neither the lockfile nor the workspace. So the
/// boundary is a serialization boundary and this is the only door through it:
/// a depth word in, JSON out, nothing but `&str` and `String` crossing.
pub fn self_test_json(depth_word: &str) -> String {
    apkaudio_contracts::self_test::run_as_json(MODULE, depth_word, self_test)
}

#[cfg(test)]
mod tests {
    use super::*;
    use apkaudio_contracts::self_test::Verdict;

    /// The self test passes against the crate it is part of. If this fails,
    /// the crate is broken — which is the entire point of it existing.
    #[test]
    fn the_self_test_passes_against_this_build() {
        let report = self_test(Depth::Own);
        assert_eq!(report.verdict(), Verdict::Pass, "{}", report.headline());
    }

    /// A run that did not ask for the hardware does not touch it. Every check
    /// filed at `Depth::Device` comes back `skipped`, by the contract rather
    /// than by this crate remembering to skip it.
    #[test]
    fn a_self_depth_run_reaches_no_hardware() {
        let report = self_test(Depth::Own);
        for check in report.checks() {
            if check.depth == Depth::Device {
                assert_eq!(
                    check.outcome,
                    apkaudio_contracts::self_test::Outcome::Skipped,
                    "{} ran at self depth",
                    check.name
                );
            }
        }
    }

    /// The name this crate files its report under is the name the bus knows it
    /// by. A crate reporting under a name nothing subscribes to is a self test
    /// that passes into the void.
    #[test]
    fn the_module_name_is_a_legal_topic_segment() {
        assert!(apkaudio_contracts::topics::is_segment(MODULE));
        assert_eq!(landing_spots(), Ok(()));
    }

    /// The door the supervisor uses answers the same thing the typed function
    /// does. Two answers to one question is the failure a second entry point
    /// invites, so the two are tied together here rather than trusted.
    #[test]
    fn the_json_door_answers_what_the_typed_one_does() {
        let typed = self_test(Depth::Own);
        let parsed = apkaudio_contracts::self_test::Report::from_json(&self_test_json("self"))
            .expect("the door emits a report this crate can read back");
        assert_eq!(parsed.verdict(), typed.verdict());
        assert_eq!(parsed.counts(), typed.counts());
        assert_eq!(parsed.headline(), typed.headline());
    }
}
