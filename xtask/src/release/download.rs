//! The download from the official server in a release: its zip, its guide,
//! and whose Bank it is for, from the numbers the download shows
//! (`bank-download-payload`). It ships with every release until the service
//! ends; this goes when patches/bank-download goes (docs/building.md).

use super::{
    guide::{anchor, row, Block, Table},
    numbers, Context, Extra,
};
use bank_download_payload::download::{
    DONE, NO_BANK_ON_SERVER, PACKAGE, PRESENT, REFUSED, WRITTEN,
};

/// The zip; it has no version in its name, as it belongs to the official
/// service, not to a version of the patch.
const NAME: &str = "offline_bank-download";
const SECTION: &str = "Pokémon still on the official server";
/// The day the official service ends, in its own time zone.
const SERVICE_ENDS: &str = "February 25, 2027";
const SERVICE_ENDS_ON: (u32, u32, u32) = (2027, 2, 25);

fn result([first, second]: [u32; 2]) -> String {
    numbers(&format!("{first:08X} {second:08X}"))
}

pub fn extra(context: &Context) -> Extra {
    let version = &context.version;
    let package = String::from_utf8_lossy(PACKAGE);
    let guide = vec![
        Block::p(format!("`{NAME}.zip` fetches the Bank you have on the official Pokémon Bank server into a new offline Bank, once, and does nothing else.")),
        Block::Lines(vec![
            format!("**Only until {SERVICE_ENDS}.** That day the official service ends, and this patch cannot do anything any more."),
            "**An offline Bank you already have is never replaced or merged.** Nothing is contacted then.".into(),
            "**Your Pokémon stay on the server as well.** This makes a copy. Poké Miles come along as the server has them.".into(),
            "**You need an internet connection and your Nintendo Network ID on this console.** No game is needed: none is chosen, read or saved.".into(),
        ]),
        Block::step("Back up Bank's extra data and your game saves with [Checkpoint](https://github.com/BernardoGiordano/Checkpoint); for Bank, press X there to switch to extdata."),
        Block::step(format!("Extract `{NAME}.zip` and copy its `luma` folder to the root of the SD card.")),
        Block::step_picture(
            format!("Start Pokémon Bank. The start screen says \"Ver. 1.5{package}\"."),
            format!("![The start screen saying Ver. 1.5{package}]({}/blob/main/pictures/download-title.png)", context.repository),
        ),
        Block::step(format!("Press START. It connects as the original does; there is no menu and nothing to choose. After the loading screens three lines appear: \"Press HOME to return to the HOME Menu\", `{version}{package}`, and two numbers. The Bank itself does not open.")),
        Block::step_table(
            "Look up the numbers:",
            Table {
                head: row!["You see", "Meaning", "What to do"],
                rows: vec![
                    row![result([DONE, WRITTEN]), "Downloaded.", "Go on with step 6."],
                    row![result([DONE, PRESENT]), "An offline Bank is already on the SD card. Nothing was contacted or changed.", "Go on with step 6."],
                    row![
                        result([REFUSED, NO_BANK_ON_SERVER]),
                        "The server holds no Bank for this console. Nothing was created or changed.",
                        format!("Go on with step 6; {version} creates a Bank."),
                    ],
                    row!["Other numbers", "Writing failed.", "Start Bank again with the download files; that is always safe. If it keeps happening, report the version line and both numbers."],
                    row!["A message of the original, then its start screen", "No connection, maintenance, or the server is locked for a while. Nothing was written.", "Try again later."],
                ],
            },
        ),
        Block::step_plain(
            format!("Install {version} as described [below]({}).", context.installing),
            &context.install,
        ),
        Block::step("Start Pokémon Bank. Your boxes are there."),
    ];
    Extra {
        kind: "download",
        name: NAME,
        heading: format!("Pokemon Bank Offline Patch: download from the official server, for {version}"),
        label: "Pokemon Bank 1.5, download from the official server".into(),
        section: SECTION,
        package: "Download package",
        guide,
        tags: Vec::new(),
        pictures: vec!["download-title.png".into()],
        routes: vec![row![
            "Pokémon on the official server and no offline Bank",
            format!("[Download them first]({}), then install {version}", anchor(SECTION)),
            format!("download them first with {NAME}.zip from this release, then install {version}"),
        ]],
        routes_after: true,
        errors: Vec::new(),
        refusal: (context.today > SERVICE_ENDS_ON).then(|| {
            format!("The official service ended on {SERVICE_ENDS}. Remove the download first (docs/building.md#the-download-from-the-official-server).")
        }),
    }
}
