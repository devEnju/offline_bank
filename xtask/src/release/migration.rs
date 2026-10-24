//! The migration from earlier releases in a release: its zip, its guide,
//! and whose Bank it is for, from the numbers the migration shows
//! (`bank-migrate-payload`). It ships with every release below v1.0.0; this
//! goes when patches/bank-migrate goes (docs/building.md).

use super::{
    guide::{anchor, row, Block, Table},
    numbers, Context, Extra,
};
use bank_migrate_payload::{
    migrate::{numbers as shown, Cause, Done, Step, Stopped, PACKAGE},
    source::Source,
};

/// The zip; it has no version in its name, as it converts whatever earlier
/// Bank it finds.
const NAME: &str = "offline_bank-migration";
const SECTION: &str = "Coming from a previous version";

fn result(result: Result<Done, Stopped<()>>) -> String {
    let [first, second] = shown(result, |_| 0);
    numbers(&format!("{first:08X} {second:08X}"))
}

pub fn extra(context: &Context) -> Extra {
    let version = &context.version;
    // The earlier ways of storing the Bank, oldest first, as only the
    // migration knows them: the last release of each, whose release stays
    // published for whoever has to settle a Bank with it, and the second
    // number of error 3 with which this release refuses such a Bank.
    let ways: Vec<(String, u32)> = Source::ALL
        .iter()
        .map(|source| {
            (
                String::from_utf8_lossy(source.tag()).into_owned(),
                source.refused(),
            )
        })
        .collect();
    let earlier = ways
        .last()
        .map(|(from, _)| format!("{from} or before"))
        .unwrap_or_default();
    let coming = anchor(SECTION);
    let package = String::from_utf8_lossy(PACKAGE);
    let converted = result(Ok(Done::Converted));
    // Who has to settle a Bank first puts back the last release of its way.
    let settle_with = format!(
        "Put back {}",
        ways.iter()
            .map(|(from, _)| format!(
                "the {} for a Bank from {from} or before",
                context.release_link(from)
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let failed = |step| shown(Err(Stopped::<()>::Failed(step, Cause::Missing)), |_| 0)[0];
    let pictures = ways
        .iter()
        .map(|(from, _)| {
            format!(
                "![The console showing a Bank of {from} converted, with {}]({}/blob/main/pictures/{from}-migration-converted.png)",
                converted.trim_matches('`'), context.repository
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let guide = vec![
        Block::p(format!(
            "`{NAME}.zip` converts a Bank that was last used with {earlier}, once, and does nothing else. After converting, that release cannot open the Bank any more; the backup of step 1 is the only way back."
        )),
        Block::step("Back up Bank's extra data with [Checkpoint](https://github.com/BernardoGiordano/Checkpoint); for Bank, press X there to switch to extdata."),
        Block::step(format!("Extract `{NAME}.zip` and copy its `luma` folder to the root of the SD card.")),
        Block::step(format!("Start Pokémon Bank. The start screen says \"Ver. 1.5{package}\".")),
        Block::step_picture(
            "Press START. After preparation three lines appear: \"Press HOME to return to the HOME Menu\", the last released version your Bank's save is from, and two numbers. The Bank itself does not open.",
            pictures,
        ),
        Block::step_table(
            "Look up the numbers:",
            Table {
                head: row!["You see", "Meaning", "What to do"],
                rows: vec![
                    row![converted, "Converted.", "Go on with step 6."],
                    row![result(Ok(Done::AlreadyNew)), "Already converted.", "Go on with step 6."],
                    row![result(Ok(Done::NoBank)), "No Bank was found.", format!("Go on with step 6; {version} creates one.")],
                    row![
                        result(Err(Stopped::InProgress)),
                        "A Save and Quit was interrupted.",
                        format!("{settle_with}; start Bank once, then begin again at step 2."),
                    ],
                    row![
                        result(Err(Stopped::Unsettled)),
                        "The Bank needs a Save and Quit first.",
                        format!("{settle_with}; open Bank with a game, Save and Quit, then begin again at step 2."),
                    ],
                    row![
                        format!(
                            "{} to {}",
                            numbers(&format!("{:08X}", failed(Step::ReadOld))),
                            numbers(&format!("{:08X}", failed(Step::WriteNew)))
                        ),
                        "A step failed.",
                        "Start Bank again with the migration files; that is always safe. If it keeps happening, restore your backup and report the version line and both numbers.",
                    ],
                ],
            },
        ),
        Block::step_plain(
            format!("Install {version} as described [below]({}).", context.installing),
            &context.install,
        ),
        Block::step_picture(
            "Start Pokémon Bank. Your boxes, Pokédex and Poké Miles are as before.",
            format!("![The boxes after the update]({}/blob/main/pictures/offline-boxes.png)", context.repository),
        ),
        Block::p("The conversion needs about 1.5 MB of free space on the SD card. If the power fails while it runs, start it again: the old Bank stays untouched until the new files are complete."),
    ];
    let mut screens: Vec<String> = ways
        .iter()
        .map(|(from, _)| format!("{from}-migration-converted.png"))
        .collect();
    screens.push("offline-boxes.png".into());
    Extra {
        kind: "migration",
        name: NAME,
        heading: format!("Pokemon Bank Offline Patch: migration from {earlier} to {version}"),
        label: format!("Pokemon Bank 1.5, migration from {earlier}"),
        section: SECTION,
        package: "Migration package",
        guide,
        tags: ways.iter().map(|(from, _)| from.clone()).collect(),
        pictures: screens,
        routes: ways
            .iter()
            .map(|(from, _)| {
                row![
                    format!("An offline Bank from {from} or before"),
                    format!("[Convert it first]({coming}), then install {version}"),
                    format!("convert it first with {NAME}.zip from this release, then install {version}"),
                ]
            })
            .collect(),
        routes_after: false,
        errors: ways
            .iter()
            .map(|(from, refused)| {
                row![
                    format!("`00000003` (with `{refused:08X}`)"),
                    format!("The Bank is from {from} or before and not converted yet."),
                    format!("[Convert it first]({coming})."),
                    format!("Convert it first with {NAME}.zip."),
                ]
            })
            .collect(),
        refusal: (context.numbers >= (1, 0, 0)).then(|| {
            "Releases from v1.0.0 on ship no migration. Remove it first (docs/building.md#the-migration-from-earlier-releases).".into()
        }),
    }
}
