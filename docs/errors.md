# Error cases and user flows

Every way a job of the normal offline patches can fail, and what the user sees and does next, with the original's game-selection and save-reading refusals the patches keep and the restore question for an interrupted Bank save. The user guides ([bank.md](bank.md), [transporter.md](transporter.md)) explain the cases; this map shows every screen and number in one place. Dialog text and page counts below describe English; the application uses the console's language. An arrow marked **A** means acknowledging the final page, after any earlier pages. Error numbers are hexadecimal. Every HOME error leads to diagram 7.

## How to read the map

- **Return to START** means the current session ends through Bank's cleanup or Transporter's closing task. The app stays open and START can be pressed again.
- **Return to selection** means Bank keeps its game-selection screen available.
- **HOME error** ends the current run. A advances to its final page but cannot dismiss it. Press HOME, close the app, and restart it.
- Every unexpected patch, storage, clock, or native-object failure in any stage goes to the shared HOME error, even where that arrow is omitted to keep the diagrams readable. Its first number identifies the stage; the table at the end covers every active fault.
- A failed write does not itself prove corruption. A later read/validation decides whether the corruption dialog appears.
- “Bank unchanged” in delivery decisions means no new delivery is announced. An unused file may have been partially written. Transporter's earlier note save may already have changed the source save, even when its Pokémon are unchanged.

## Dialog key: exact English text and pages

| Name used in diagrams | Full text, in page order | Pages and original message |
| --- | --- | --- |
| Retry | “Please confirm that your Game Card is properly inserted, and then try again.” | 1 displayed page; Bank 39:0x0B or Transporter 23:0x0B, second part only |
| Full SD | “The SD card is full.” | 1 displayed page; 26:0x00, first line only |
| Bank startup space | “Pokémon Bank could not be started due to a lack of space on your SD card.” | 1; 39:0x26 |
| Corrupt save | “The saved data in your Pokémon game is corrupted.” | 1; 23:0x02, also available in Bank's text archive |
| Waiting for game | “You previously used a different copy of this game. Please try again using [name]’s game (Trainer ID No. [number]).” | 1; 39:0x0A; name/ID come from the pending save |
| Open Transporter | **1:** “Your communication with Poké Transporter did not complete correctly during your last session.” **2:** “Please open Poké Transporter and perform the cleanup process before using Pokémon Bank.” | 2; 39:0x05 |
| Restore question | “You can try restoring your data, but you may lose the Pokémon currently being transported. Would you still like to try restoring your data?” | 1 with Yes/No buttons; 23:0x1A |
| Restore loading | “Communication in your last session did not complete correctly. Performing cleanup process now...” | 1 loading panel, no acknowledgement; 23:0x04 |
| No Bank game | **1:** “Could not find saved data for a game that is compatible with Pokémon Bank.” **2:** “You must also advance in the game to the point where you have received a Pokédex.” | 2 displayed pages; 39:0x00, first two pages only |
| Pokédex required | “Before you can use Pokémon Bank, you need to get the Pokédex in your Pokémon game.” | 1; 39:0x1A |
| Old save mismatch | “The saved data in this software does not match the previous saved data. It cannot be used.” | 1; 39:0x04 |
| Bank no card | **1:** “No Pokémon Game Card detected.” **2:** “Please check that your Game Card is properly inserted into your system’s Game Card slot.” | 2; 39:0x1F |
| No Transporter game | “Could not find a game that is compatible with Poké Transporter.” | 1; 23:0x00 |
| No source save | “No saved data found for a Pokémon game.” | 1; 23:0x24 |
| Transporter no card | **1:** “No compatible Pokémon Game Card detected.” **2:** “Please check that your Game Card is properly inserted into your system’s Game Card slot.” | 2; 23:0x22 |
| Open Bank | **1:** “Your communication with Pokémon Bank did not complete correctly during your last session.” **2:** “Please open Pokémon Bank and perform the cleanup process before using Poké Transporter.” | 2; 23:0x03 |
| Empty Transport Box | **1:** “At least one Pokémon remains in the Transport Box from your previous session.” **2:** “Please empty the Transport Box by using Pokémon Bank before using Poké Transporter.” | 2; 23:0x05 |
| No Pokémon | “No Pokémon found to move. Please put your Pokémon in the [box name] Box and try again.” | 1; 23:0x08; the box name is substituted by the original |
| Bank success | “Success! Changes saved.” | 1; 39:0x09 |
| Transporter success | **1:** “Pokémon moved to Pokémon Bank’s Transport Box.” **2:** “Please open Pokémon Bank to check the Pokémon in your Transport Box.” | 2; 23:0x0A; this is the normal success path |
| HOME error | **1:** “The server did not receive the data.” **2:** “Press HOME to return to the HOME Menu”, with the patch version/package on the next line and both eight-digit numbers below it. | 2 composed pages; Bank 39:0x0B first part + 39:0x4D; Transporter 23:0x0B first part + 23:0x27 |
| HOME instruction alone | “Press HOME to return to the HOME Menu” | 1 page, without version or numbers; fallback when the complete error text cannot be composed |

The HOME error uses the original server sentence as a general failure sentence. It does not mean that an offline server request was attempted. The English HOME button glyph is written as HOME here. The transformed retry and HOME dialogs also handle Simplified Chinese, where the original separates the parts by a line rather than a page.

## 1. Bank: startup and choosing a game

```mermaid
flowchart TD
    START(["Bank: press START"]) --> SCAN{"Scan games and mount storage"}
    SCAN -->|"Insufficient space for native storage setup"| SPACE["Pokémon Bank could not be started due to a lack of space on your SD card.<br/>1 page"]
    SPACE -->|"A; free space before retrying"| TITLE(["Bank start screen"])
    SCAN -->|"No readable compatible save"| NONE["Could not find saved data for a game that is compatible with Pokémon Bank.<br/>↓ A<br/>You must also advance in the game to the point where you have received a Pokédex.<br/>2 pages"]
    NONE -->|"A"| TITLE
    SCAN -->|"Compatible save found"| OPEN{"Open offline Bank"}
    OPEN -->|"Cannot open files"| F3["HOME error: 03"]
    OPEN -->|"Invalid stored state or journal"| F5["HOME error: 05"]
    OPEN -->|"Current Bank data cannot be loaded"| F9["HOME error: 09"]
    OPEN -->|"Bank does not exist yet"| CREATE["Welcome and create Bank"]
    CREATE -->|"Insufficient space"| SPACE
    CREATE -->|"Other creation failure"| F4["HOME error: 04"]
    CREATE -->|"Created"| SELECT["Game selection"]
    OPEN -->|"Pending save"| RECOVER["Recovery flow — diagram 2"]
    RECOVER -->|"Resolved"| SELECT
    RECOVER -->|"Recoverable refusal acknowledged"| TITLE
    OPEN -->|"Clean Bank loaded"| SELECT
    SELECT -->|"Pokédex not obtained"| DEX["Before you can use Pokémon Bank, you need to get the Pokédex in your Pokémon game.<br/>1 page"]
    DEX -->|"A"| SELECT
    SELECT -->|"Native secure-value comparison rejects save"| OLD["The saved data in this software does not match the previous saved data. It cannot be used.<br/>1 page"]
    OLD -->|"A"| TITLE
    SELECT -->|"Native console query fails"| CARD["No Pokémon Game Card detected.<br/>↓ A<br/>Please check that your Game Card is properly inserted into your system’s Game Card slot.<br/>2 pages"]
    CARD -->|"A"| TITLE
    SELECT -->|"Accepted"| VALIDATE{"Validate loaded save"}
    VALIDATE -->|"Checksum or signature fails"| CORRUPT["The saved data in your Pokémon game is corrupted.<br/>1 page"]
    CORRUPT -->|"A; restore save before using that game again"| TITLE
    VALIDATE -->|"Valid"| LOAD{"Read game and secure value"}
    LOAD -->|"First query fails before medium is known"| RETRY["Please confirm that your Game Card is properly inserted, and then try again.<br/>1 page"]
    LOAD -->|"Known cartridge disappears"| RETRY
    RETRY -->|"A"| TITLE
    LOAD -->|"Other read or secure-value failure"| FAE["HOME error: 0A or 0E"]
    LOAD -->|"Loaded"| REWARD{"Reward guard and choice"}
    REWARD -->|"Continue"| BOXES(["Bank boxes"])
    REWARD -->|"Decline to continue"| TITLE
    REWARD -->|"Invalid reward state or receipt"| FR["HOME error: 12 or 13"]
    SELECT -->|"Back"| TITLE
```

The first-query exception applies at the patch's load, before the query has identified cartridge versus installed copy. The earlier native selection has its own no-card dialog. A failure later in a load is treated as a missing card only for a known cartridge; an installed game's failure gets numbers.

An inserted cartridge takes precedence over an installed copy of the same title. For a pending installed save hidden by such a cartridge, remove the cartridge before START. For a pending cartridge save, insert it. Exact matching save images can identify the pending transaction on either medium; a different image on the other medium does not substitute for its missing game.

If **no readable compatible game at all** is available, the scan stops before pending recovery. The waiting-for-game dialog therefore requires the scan to have found some readable compatible game.

## 2. Bank: recovering a pending save at START

```mermaid
flowchart TD
    PENDING(["Pending Bank save found"]) --> AVAILABLE{"Expected game available?"}
    AVAILABLE -->|"Available"| READ{"Read and identify its save"}
    READ -->|"Cartridge disappears"| RETRY["Please confirm that your Game Card is properly inserted, and then try again.<br/>1 page"]
    RETRY -->|"A; reinsert and press START"| TITLE(["Bank start screen"])
    READ -->|"Other read or transaction failure"| FATAL["HOME error: 0A, 0B or 0E"]
    READ -->|"Exact before-image"| BEFORE["Check required secure value"]
    BEFORE -->|"Accepted"| KEEP["Keep old Bank"]
    BEFORE -->|"Fails"| FATAL
    READ -->|"Exact after-image"| AFTER["Accept or finish required secure value"]
    AFTER -->|"Accepted"| COMMIT["Commit new Bank"]
    AFTER -->|"Fails"| FATAL
    READ -->|"Neither image"| MEDIUM{"Expected title and medium?"}
    MEDIUM -->|"Yes"| PRESENT{"What moved?"}
    PRESENT -->|"Deposits only"| COMMIT
    PRESENT -->|"Withdrawals/releases only, or no moves"| KEEP
    PRESENT -->|"Deposits and withdrawals"| ASK{"Leave pending save to<br/>Transporter's restore question"}
    ASK -->|"Journal update fails"| F10["HOME error: 10"]
    ASK -->|"Recorded, or already recorded"| TRANSPORTER["Your communication with Poké Transporter did not complete correctly during your last session.<br/>↓ A<br/>Please open Poké Transporter and perform the cleanup process before using Pokémon Bank.<br/>2 pages"]
    TRANSPORTER -->|"A"| TITLE
    TRANSPORTER -.->|"After A: close Bank; open Transporter and press START"| RESTORE(["Restore flow — diagram 5"])
    MEDIUM -->|"Different medium with a different save"| ABSENT{"What moved?"}
    AVAILABLE -->|"Game unavailable"| ABSENT
    ABSENT -->|"Deposits only"| COMMIT
    ABSENT -->|"Withdrawals/releases only, or no moves"| KEEP
    ABSENT -->|"Deposits and withdrawals"| TAKEBACK{"Leave pending; take back any<br/>Transporter restore question"}
    TAKEBACK -->|"Journal update fails"| F10
    TAKEBACK -->|"Recorded, or already recorded"| WAIT["You previously used a different copy of this game.<br/>Please try again using [name]’s game<br/>(Trainer ID No. [number]).<br/>1 page"]
    WAIT -->|"A; supply expected game and press START"| TITLE
    KEEP --> WRITE{"Finish recovery and load Bank"}
    COMMIT --> WRITE
    WRITE -->|"Recovery write fails"| F10
    WRITE -->|"Bank readback fails"| F9["HOME error: 09"]
    WRITE -->|"Successful"| SELECT(["Game selection"])
```

| What can be established | Bank result | Next user step |
| --- | --- | --- |
| Game is exactly the before-image and its secure value is acceptable | Keep old Bank | Game selection |
| Game is exactly the after-image and its secure value can be accepted/finished | Commit new Bank | Game selection |
| Neither exact image; expected title and medium are present; deposits only | Commit new Bank | Game selection; a damaged game is refused if selected |
| Neither exact image; expected title and medium are present; only withdrawals/releases, or no moves | Keep old Bank | Game selection; a damaged game is refused if selected |
| Neither exact image; expected title and medium are present; deposits and withdrawals | Leave pending and record that Transporter must ask | Two-page instruction → A → Bank start screen; then manually close Bank and open Transporter for diagram 5 |
| Game missing or different medium with a different image; deposits only | Commit new Bank | Game selection |
| Game missing or different medium with a different image; withdrawals/releases only, or no moves | Keep old Bank | Game selection |
| Game missing or different medium with a different image; deposits and withdrawals | Leave pending; take back any Transporter question | Waiting for game → A → START |
| Cartridge disappears during recovery | Leave pending | Retry → A → START |
| Game read, transaction binding, secure-value operation, recovery write, or Bank readback fails otherwise | Stop the run | HOME error |

Recovery from one-way moves is a deliberate policy: deposits can be duplicated when new Bank is kept, and withdrawals can be duplicated when old Bank is retained. A mixed-move session with neither exact game image is never settled automatically. With the expected title and kind of copy present, Bank records that Transporter must ask the user; otherwise Bank waits for the game and takes back any earlier question. Recording or taking back the question changes only the journal, not the Bank snapshots or the game save. Recovery runs before the new chosen-game corruption check.

Repeated Bank START with the same nonmatching game repeats the message to open Transporter. If an exact before/after image becomes available, Bank can settle it itself. If the game is absent or only a nonmatching copy on the other medium is available, Bank waits instead. Taking out one cartridge may expose an installed copy of the same title; Bank then treats that other medium as unavailable for the pending save.

## 3. Bank: Save and Quit

```mermaid
flowchart TD
    START(["Save and Quit"]) --> DETECT{"Initial game query"}
    DETECT -->|"Fails: cartridge"| RETRY["Please confirm that your Game Card is properly inserted, and then try again.<br/>1 page"]
    DETECT -->|"Fails: installed game"| FA["HOME error: 0A"]
    DETECT -->|"Passes"| PREP["Count Pokémon, settle rewards,<br/>prepare game and Bank"]
    PREP -->|"Prepared game invalid"| FC["HOME error: 0C"]
    PREP -->|"Game image or medium changed"| FB["HOME error: 0B"]
    PREP -->|"Secure value unacceptable"| FE["HOME error: 0E"]
    PREP -->|"Cartridge disappears before write"| RETRY
    PREP -->|"Missing Bank file cannot be created: no space"| FULL["The SD card is full.<br/>1 page"]
    PREP -->|"Other Bank preparation failure"| FF["HOME error: 0F"]
    PREP -->|"Journal durable; final checks pass"| WRITE["Original writer saves game"]
    WRITE -->|"Cannot start or unexpected native state"| FD["HOME error: 0D"]
    WRITE -->|"Writer ends, including a reported write failure"| INSPECT{"Inspect what was actually saved"}
    INSPECT -->|"Complete new image with prepared secure pair"| VERIFY{"Commit and verify Bank"}
    VERIFY -->|"Bank matches staged data"| SUCCESS["Success! Changes saved.<br/>1 page"]
    VERIFY -->|"Bank readback fails or differs"| F9["HOME error: 09"]
    VERIFY -->|"Settlement write fails"| F10["HOME error: 10"]
    INSPECT -->|"Cartridge absent"| PENDING["Leave save pending"]
    PENDING --> RETRY
    INSPECT -->|"Old image"| OLD["Keep old Bank"]
    INSPECT -->|"Neither image; deposits only"| NEW["Commit new Bank"]
    INSPECT -->|"Neither image; withdrawals/releases only, or none"| OLD
    INSPECT -->|"Neither image; mixed moves"| MIXED["Leave pending for next START"]
    OLD --> MEDIUM{"Cartridge or installed?"}
    NEW --> MEDIUM
    MIXED --> MEDIUM
    MEDIUM -->|"Cartridge"| RETRY
    MEDIUM -->|"Installed"| FD
    INSPECT -->|"Other read or secure-value failure"| FAE["HOME error: 0A or 0E"]
    OLD -->|"Settlement fails"| F10
    NEW -->|"Settlement fails"| F10
    RETRY -->|"A"| TITLE(["Bank start screen"])
    FULL -->|"A; free space before retrying"| TITLE
    SUCCESS -->|"A"| TITLE
```

After the writer ends, its status alone does not determine success. The patch reads what was actually saved:

| Observed game | Bank decision | Cartridge message | Installed-game message |
| --- | --- | --- | --- |
| Exact new image with the secure pair this save prepared, followed by matching Bank readback | Commit | Bank success | Bank success |
| Exact old image | Undo | Retry | HOME error, first number D |
| Neither image; only deposits | Commit new Bank | Retry | HOME error, first number D |
| Neither image; only withdrawals/releases, or no moves | Keep old Bank | Retry | HOME error, first number D |
| Neither image; mixed moves | Leave pending for next START | Retry | HOME error, first number D |
| Cartridge absent/unreadable because it was removed | Leave pending | Retry | Not a cartridge-removal case |

An interrupted preparation attempts to undo any pending record before it returns. If that undo cannot finish, next START finds the pending save and resolves it against the untouched game. An error while settling may also leave a pending record. The corruption dialog is reserved for a later validation failure, not inferred merely from an unsuccessful write.

A mixed-move save whose result matches neither image is left pending after the writer ends. The subsequent Bank START decides whether it can settle an exact image, must wait for the game, or must send the player to Transporter's restore question (diagram 2).

## 4. Transporter: startup, reading, and the save before the question

```mermaid
flowchart TD
    START(["Transporter: press START"]) --> CHECK{"Can Bank accept a delivery?<br/>Full readiness check happens here"}
    CHECK -->|"Transport Box occupied or delivery waiting"| OCC["At least one Pokémon remains in the Transport Box from your previous session.<br/>↓ A<br/>Please empty the Transport Box by using Pokémon Bank before using Poké Transporter.<br/>2 pages"]
    CHECK -->|"No Bank, unfinished save, old format,<br/>missing or repairable file"| OPEN["Your communication with Pokémon Bank did not complete correctly during your last session.<br/>↓ A<br/>Please open Pokémon Bank and perform the cleanup process before using Poké Transporter.<br/>2 pages"]
    CHECK -->|"Bank data cannot be opened for a reason Bank cannot resolve"| F3["HOME error: 03"]
    CHECK -->|"Damaged or conflicting journal"| F5["HOME error: 05"]
    CHECK -->|"Bank left a pending save to the restore question"| RESTORE(["Restore flow — diagram 5<br/>No game search or source save"])
    OCC -->|"A; resolve in Bank"| TITLE(["Transporter start screen"])
    OPEN -->|"A; resolve in Bank"| TITLE
    CHECK -->|"Ready"| SEARCH{"Find compatible games"}
    SEARCH -->|"None"| NONE["Could not find a game that is compatible with Poké Transporter.<br/>1 page"]
    NONE -->|"A"| TITLE
    SEARCH -->|"Found"| LIST["Choose game"]
    LIST --> READ{"Original source reader"}
    LIST -->|"Back or cancel"| TITLE
    READ -->|"Cartridge removed"| CARD["No compatible Pokémon Game Card detected.<br/>↓ A<br/>Please check that your Game Card is properly inserted into your system’s Game Card slot.<br/>2 pages"]
    READ -->|"No save"| NOSAVE["No saved data found for a Pokémon game.<br/>1 page"]
    READ -->|"Corruption detected"| CORRUPT["The saved data in your Pokémon game is corrupted.<br/>1 page"]
    CARD -->|"A"| TITLE
    NOSAVE -->|"A"| TITLE
    CORRUPT -->|"A; restore save before retrying"| TITLE
    READ -->|"Read successfully"| NOTE{"Save original note before question"}
    NOTE -->|"Cartridge missing or cartridge write fails"| RETRY["Please confirm that your Game Card is properly inserted, and then try again.<br/>1 page"]
    NOTE -->|"SD backup lacks space before source is touched"| FULL["The SD card is full.<br/>1 page; text fallback — diagram 7"]
    NOTE -->|"Other SD backup/save failure<br/>or possibly partial source write"| FD["HOME error: 0D + result"]
    NOTE -->|"Virtual Console save fails"| FVC["HOME error: 0D + zero"]
    RETRY -->|"A"| TITLE
    FULL -->|"A; free space before retrying"| TITLE
    NOTE -->|"Saved"| FILTER["Original Pokémon refusal notices<br/>Filter out ineligible Pokémon"]
    FILTER --> ELIGIBLE{"Any eligible Pokémon?"}
    ELIGIBLE -->|"No"| EMPTY["No Pokémon found to move.<br/>Please put your Pokémon in the [box name] Box and try again.<br/>1 page"]
    EMPTY -->|"A"| TITLE
    ELIGIBLE -->|"Yes"| QUESTION{"Move this Box?"}
    QUESTION -->|"No"| TITLE
    QUESTION -->|"Yes"| DELIVERY(["Delivery flow — diagram 6"])
```

The two Bank-readiness dialogs appear only at START. A pending save Bank left to the restore question goes directly to diagram 5, before any compatible-game search or note save. Transporter follows Bank's journal here; it does not inspect the Bank session's game itself. A pending save Bank has not left to that question still produces the message to open Bank.

The note save happens before the transfer question. It may change the source save even if the player later answers No; no delivery has happened at that point. A cartridge write failure at this stage retains the one-page retry advice. On the SD-save path, a verified `.sav.bak` is required before the source is written. A failed backup can leave a partial `.sav.bak`, while the source stays untouched. Once a source write has been attempted, lack of space is not presented as the safe, untouched-backup case.

### Retained Pokémon refusal sequence

These notices filter the transfer rather than producing a numbered error. Each is one page. When refusal flags are present, the original shows the introduction, one notice for each applicable reason, then the removal notice. Remaining eligible Pokémon can still be offered.

| Step | Full English text and original message |
| --- | --- |
| Introduction | “Attention! There is at least one Pokémon in the Box that can’t be sent.” (23:0x11) |
| Egg | “A Pokémon Egg has been detected. Sorry. You can’t send Pokémon Eggs.” (23:0x12) |
| Fused Kyurem | “Black Kyurem or White Kyurem has been detected. Sorry. Kyurem can be sent only in its unfused form.” (23:0x13) |
| Problematic Pokémon | “A problem has been detected with one of the Pokémon. Sorry. The problematic Pokémon can’t be sent.” (23:0x14) |
| Held-item refusal | “There are many Pokémon that are holding an item. Pokémon that are holding an item can’t be moved.” (23:0x41) |
| End of refusal sequence | “Any Pokémon that could not be transported have been removed from the Transport Box.” (23:0x15) |

“Removed from the Transport Box” here describes the proposed transfer in memory; refused Pokémon stay in the source game. The separate held-item notice, when applicable, is “An item held by a Pokémon can’t be moved with it. Any items held by Pokémon have been returned to [name]’s Bag in the game.” (23:0x16, one page).

## 5. Transporter: answering Bank's restore question

```mermaid
flowchart TD
    START(["Transporter START finds a pending save<br/>Bank left to its restore question"]) --> QUESTION{"You can try restoring your data, but you may lose the Pokémon currently being transported. Would you still like to try restoring your data?<br/>1 page; Yes and No buttons"}
    QUESTION -->|"HOME; close Transporter before answering"| LATER(["No choice applied;<br/>decide later"])
    QUESTION -->|"Yes"| BEFORE["Choose Bank's boxes<br/>from before the interrupted session"]
    QUESTION -->|"No"| AFTER["Choose Bank's boxes<br/>from after the interrupted session"]
    BEFORE --> PANEL["Communication in your last session did not complete correctly. Performing cleanup process now...<br/>Loading panel; HOME and sleep refused"]
    AFTER --> PANEL
    PANEL --> SETTLE{"Settle Bank's journal<br/>to the chosen snapshot"}
    SETTLE -->|"Required Bank file cannot be opened"| F3["HOME error: 03"]
    SETTLE -->|"Damaged or conflicting journal"| F5["HOME error: 05"]
    SETTLE -->|"Other settlement failure,<br/>including insufficient space or changed state"| F10["HOME error: 10 + result or zero"]
    SETTLE -->|"Completed"| CLOSE["Finish loading;<br/>Transporter's closing screen"]
    CLOSE --> TITLE(["Transporter start screen"])
    TITLE -.->|"Close Transporter; open Bank and press START"| BANK(["Bank opens the selected snapshot;<br/>normal startup checks apply"])
```

Yes and No both settle the pending save. No does not cancel the question. Closing Transporter with HOME before answering leaves it for later. Only the first restore question is shown; there is no second confirmation question. The loading panel follows either answer, then the normal closing screen and start screen; Transporter does not continue to game selection in that session.

Only Bank's journal is written. This chooses which existing Bank snapshot is current; it does not restore or write the game save, transfer Pokémon, or create a source backup. The question can be answered without the original game present in Transporter, because Transporter relies on the choice Bank recorded. Until the question is answered, Bank takes it back when it next checks the pending save itself and the game is gone or only on the other kind of copy (diagram 2); an answer, once given, is final.

| Pokémon currently in the game from the interrupted session | Answer | Bank afterwards |
| --- | --- | --- |
| The Pokémon deposited into Bank in that session | Yes | Old Bank boxes, retaining the Pokémon withdrawn from Bank |
| The Pokémon withdrawn from Bank in that session | No | New Bank boxes, retaining the Pokémon deposited into Bank |
| Neither, or uncertain | Decide after backing up the game save and Bank together | A wrong choice can lose Pokémon; paired backups allow the choice to be undone |

If a Transporter delivery was waiting before the interrupted Bank session, Yes leaves it waiting; No accepts it as part of the after-snapshot and clears the waiting-delivery record. On a later Transporter START the ordinary readiness check applies again. Settlement failures use Bank's storage/recovery numbers, including `10` for insufficient space; they do not use the ordinary full-card notice or delivery error `14`. After an error, restart re-reads the durable journal.

## 6. Transporter: delivery and the saves after delivery

```mermaid
flowchart TD
    YES(["Answer Yes"]) --> CARD{"Chosen cartridge already removed?"}
    CARD -->|"Yes"| RETRY["Please confirm that your Game Card is properly inserted, and then try again.<br/>1 page"]
    RETRY -->|"A; no delivery"| TITLE(["Transporter start screen"])
    CARD -->|"No; also SD and VC sources"| DELIVER{"Read Bank journal;<br/>write and verify delivery<br/>No repeated Transport Box scan"}
    DELIVER -->|"Journal no longer permits delivery"| REFUSED["HOME error:<br/>00000014 00000005"]
    DELIVER -->|"Cannot open required Bank file"| F3["HOME error: 03"]
    DELIVER -->|"Damaged or conflicting journal"| F5["HOME error: 05"]
    DELIVER -->|"Confirmed no delivery: insufficient space"| FULL["The SD card is full.<br/>1 page; text fallback — diagram 7"]
    DELIVER -->|"Confirmed no delivery: other IO/write/box failure"| F14["HOME error: 14<br/>Second number identifies the failure"]
    DELIVER -->|"Announcement fails"| AGAIN{"Read journal again:<br/>does Bank hold this delivery?"}
    AGAIN -->|"Delivery confirmed"| SAVES["Bank holds Pokémon;<br/>perform original source-save steps"]
    AGAIN -->|"Confirmed absent"| CAUSE{"Original failure"}
    CAUSE -->|"Insufficient space"| FULL
    CAUSE -->|"Other failure"| F14
    AGAIN -->|"Still uncertain"| UNCERTAIN["HOME error:<br/>00000014 00000004<br/>Source Pokémon are retained;<br/>Bank may also hold them"]
    DELIVER -->|"Delivery confirmed"| SAVES
    SAVES -->|"Cartridge removed before a source save"| NOCARD["No compatible Pokémon Game Card detected.<br/>↓ A<br/>Please check that your Game Card is properly inserted into your system’s Game Card slot.<br/>2 pages"]
    SAVES -->|"SD source save fails"| FD["HOME error: 0D + result"]
    SAVES -->|"Cartridge write fails; card still present"| FD
    SAVES -->|"VC source save fails"| FVC["HOME error: 0D + zero"]
    SAVES -->|"All save steps finish"| SUCCESS["Pokémon moved to Pokémon Bank’s Transport Box.<br/>↓ A<br/>Please open Pokémon Bank to check the Pokémon in your Transport Box.<br/>2 pages"]
    FULL -->|"A; free space before retrying"| TITLE
    NOCARD -->|"A; inspect Bank before repeating"| TITLE
    SUCCESS -->|"A"| TITLE
```

| Failure point | Bank's delivery | Source Pokémon | Why this destination |
| --- | --- | --- | --- |
| Removed cartridge before delivery | None | Still in source | Retry can start a fresh session |
| Journal no longer permits delivery | None | Still in source | HOME error `14 / 05`; close Transporter and inspect Bank before another transfer |
| Delivery is confirmed absent for lack of space | None | Still in source | Full SD; free space before retrying |
| Delivery is confirmed absent for another unexpected reason | None | Still in source | HOME error `03`, `05`, or `14`, according to the failure |
| Delivery announcement remains uncertain after reread | May exist | Still in source | HOME error `14 / 04`; close Transporter and inspect Bank before another transfer |
| Delivery durable; cartridge removed before saving source | Exists | Still in source at that point | Original no-card explanation; Bank must be checked before repeating |
| Delivery durable; source save fails otherwise | Exists | May still be present; save may be damaged | HOME error `0D`; close Transporter and inspect Bank before another transfer |
| Everything finishes | Exists | Removed from source | Normal success, which also directs the player to Bank |

Delivery reads the journal to choose the unused transport-box copy and ensure delivery is still permitted. It does not repeat START's scan of that copy before writing it. The delivery is written, flushed, read back and announced before the source-removal steps can begin. If the announcement fails, the journal is read again to determine whether Bank holds the delivery.

The original saves the source more than once after delivery, including its note handling. A late save failure can occur after Pokémon were already removed successfully. “May still be present” is intentional; the dialog does not claim that duplication definitely occurred. An SD source-save failure after delivery leads to HOME error `0D` even if the underlying result concerns space, because the delivery has already happened. Neither Bank-readiness dialog appears during delivery or the subsequent source saves.

## 7. Shared HOME screen and exceptional fallback paths

```mermaid
flowchart TD
    FAIL(["Failure requiring a HOME error"]) --> JOB{"Worker job still running?"}
    JOB -->|"Yes"| WAIT["Keep loading screen;<br/>HOME and sleep remain refused"]
    WAIT --> JOB
    JOB -->|"No"| WRITER{"Bank: native game writer<br/>still running?"}
    WRITER -->|"Yes"| WAITWRITER["Wait for writer to finish;<br/>HOME and sleep remain refused"]
    WAITWRITER --> WRITER
    WRITER -->|"No; or Transporter"| STOP["Stop loading;<br/>allow HOME and sleep"]
    STOP --> UI{"Usable error-dialog UI?"}
    UI -->|"Yes"| BUILD{"Can the complete error text<br/>and HOME page be composed?"}
    BUILD -->|"Yes"| PAGE1["The server did not receive the data.<br/>Page 1"]
    PAGE1 -->|"A"| PAGE2["Press HOME to return to the HOME Menu<br/>Patch version/package<br/>XXXXXXXX YYYYYYYY<br/>Page 2"]
    BUILD -->|"No: text unavailable or insufficient room"| HOMEONLY["Press HOME to return to the HOME Menu<br/>1 page; instruction alone<br/>No version or numbers"]
    PAGE2 -->|"A, B or START: remain on error screen"| PAGE2
    HOMEONLY -->|"A, B or START: remain on error screen"| HOMEONLY
    PAGE2 -->|"HOME; close application"| RESTART(["Restart application"])
    HOMEONLY -->|"HOME; close application"| RESTART
    RESTART --> NEXT["Next Bank START recovers<br/>any pending Bank save"]
    UI -->|"No usable UI"| END["Task follows its failure/cleanup path;<br/>no reliable error dialog<br/>No forced application exit"]
    TFULL(["Transporter needs full-card dialog"]) --> TEXT{"Can its message file be opened?"}
    TEXT -->|"Yes"| FULL["The SD card is full.<br/>1 page"]
    FULL -->|"A"| TITLE(["Transporter start screen"])
    TEXT -->|"No"| F2["HOME error: 02"]
    F2 --> JOB
    BNOTICE(["Bank cannot construct a required notice"]) --> F2
```

The normal error has two pages. If the complete text cannot be composed, the original HOME instruction is shown alone, without a partial version or number line. If the UI itself is unavailable, the task follows its failure/cleanup path; the application is not forcibly closed.

“The SD card is full.” comes from a separate application message file, opened temporarily on the UI's heap. Failure to obtain that object is an exceptional internal/resource failure and leads to HOME error `02`, rather than cartridge advice. Having a full SD card does not itself prevent reading this existing text and requires no free space to display it. A numbered error is a final stop for that run, not an ordinary dialog to dismiss and retry in place.

### Every active first error number

Numbers are hexadecimal; the screen pads each to eight digits. The second number preserves a native result where available, otherwise a local diagnostic or zero. A zero second number does not mean success.

| First number | Cases routed to the HOME error |
| --- | --- |
| `01` | Conflicting/reentrant use of a patch job or state |
| `02` | Unexpected native task/UI/data object; worker start, capacity or lifecycle failure; required UI/panel setup failure; Transporter's full-card message file cannot be opened |
| `03` | Bank archive/files cannot be opened; Transporter uses this where opening Bank cannot resolve the access failure |
| `04` | Bank creation fails for a reason other than recognized lack of space |
| `05` | Invalid stored state or damaged/conflicting journal records |
| `06` | Console clock cannot provide an acceptable value |
| `09` | Stored Bank cannot be loaded, restored, or verified; final Bank readback differs from staged data |
| `0A` | Game save cannot be read, or installed-game detection fails; known cartridge-removal cases use Retry instead |
| `0B` | Game image, title/medium, or transaction binding does not match the expected session |
| `0C` | Prepared game image or its geometry/checks are invalid; failure is before the native game write |
| `0D` | Native game write cannot complete as required; Bank's installed-game result is not the complete prepared save; Transporter's SD/VC save fails before the question or after delivery; cartridge write fails after delivery while the card remains present |
| `0E` | Secure-value mismatch, malformed secure-value result, or required secure-value operation fails, after cartridge-removal exceptions |
| `0F` | Preparing Bank's side files/journal fails unexpectedly; recognized no-space cases use Full SD |
| `10` | Finishing/undoing/reconciling a pending Bank save fails, including Transporter's settlement of the restore answer |
| `12` | Reward state or allowed reward-task progression is inconsistent |
| `13` | Reward claim result does not match the expected receipt/balance |
| `14` | Transporter delivery failure: non-space IO failure, write/check failure, invalid box layout, unexpectedly empty transfer box, journal refusal after START, or uncertain delivery announcement |

### Second numbers

| Second number | Meaning |
| --- | --- |
| Small number, for example `00176BF0` | A file exists with that size instead of the expected one. |
| `78000000` | Bank's own filesystem session was not ready. |
| `710000oo` | A call succeeded but returned no handle (`oo`: 0 open archive, 3 open file). |
| `720000oo` | A short read or write (`oo`: 5 read, 6 write). |
| `79000000` | A file that is needed exists but cannot be opened. |
| `7A00ffpp` | A problem with one of the files. `ff`: 0 boxes, 1 main, 2 transport. `pp`: 1 damaged or missing, 3 Pokémon from Poké Transporter wait but cannot be read, so nothing is saved over them, 4 not the format this version knows, 5 not ready: the boxes are not open, or a save is still in progress. |
| `8xxxxxxx`–`Fxxxxxxx` | The console's own filesystem result code. |

Transporter's ordinary active set is `02`, `03`, `05`, `0D`, `10`, `14`; `10` covers failure to settle the restore answer. It does not translate all of Bank's faults into its own dialogs. Bank can show the other numbers when Transporter sends the player there. The low-level thread result is a second number under `02`, not a separate user-facing dialog.

For `14`, the second number distinguishes the following outcomes. The native question flow is supposed to prevent malformed/empty boxes reaching delivery.

| Second number | Meaning | Outcome of this delivery |
| --- | --- | --- |
| `00000001` | Invalid transport-box layout | No delivery; source Pokémon retained |
| `00000002` | Empty transport box unexpectedly submitted | No delivery |
| `00000003` | Required delivery write or verification failed | No delivery; source Pokémon retained |
| `00000004` | Delivery announcement could not be confirmed | Bank may hold it; source Pokémon retained |
| `00000005` | Journal no longer permits delivery | No delivery; source Pokémon retained |
| Console result | Other storage IO failure | No delivery; source Pokémon retained |

## Retained Bank reward and box restrictions

These are local refusals or choices, rather than patch failures. They do not produce the HOME error merely because the user encounters them.

| Situation | Dialog / result |
| --- | --- |
| Gen 6 pending Pokémon Link gift blocks another reward | **1:** “[name]’s Pokémon Link (ID No.: [number]) is full.” **2:** “Please receive the previous present via Pokémon Link in your Game Card.” **3:** “Even though a new present is waiting for you, you won’t be able to receive it this time.” **4:** “Do you still want to go ahead and use Pokémon Bank?” (39:0x07, 4 pages). Continue into boxes without that claim, or decline and return to START. |
| Gen 7 Mystery Gift store full during the reward guard | **1:** “[name]’s Mystery Gift (ID No. [number]) is full.” **2:** “Please check your Card Album for your Mystery Gift and throw away an old Wonder Card to receive a gift.” **3:** “Otherwise, you can’t receive a gift this time even if there is a new gift. Do you still want to go ahead and use Pokémon Bank?” (39:0x51, 3 pages). Continue into boxes without that claim, or decline and return to START. |
| Local Pokémon/box action refused | The original restriction notice is acknowledged, then the user remains in the box UI and can choose another action. The failed action does not save or end the session. |

The retained box restriction texts include “You cannot deposit an Egg.” (38:0x1C), “You cannot deposit this Pokémon.” (38:0x1D), “You cannot release an Egg.” (38:0x1E), “You cannot release this Pokémon.” (38:0x1F), “Cannot be transported there.” (38:0x23), “At least one Pokémon is in the Transport Box.” (38:0x24), “You cannot use this function.” (38:0x25), “You cannot check the status of an Egg.” (38:0x26), “Pokémon in a locked team can’t be moved.” (38:0x28), “You cannot move that Pokémon here.” (38:0x29), “No Box has been registered.” (38:0x32), “You cannot register this Box.” (38:0x34), “It is already registered to this group.” (38:0x35), and “One or more can’t be moved to this game.” (38:0x36). Each text is one English page; its applicability remains the original UI's decision.

## Boundaries and evidence

This describes the normal offline runtime after successful application startup. Unmodified executable/update-data errors, system crash screens, and the migration/download/test-build result flows have their own startup or utility behavior. A mismatched patch pair or unsupported executable can prevent reaching any mapped flow.

The map follows the code, the original's own selection code, its English texts and [internals.md](internals.md); what a console shows is checked with [testing.md](testing.md). Relevant implementation:

- [Bank hooks](../patches/bank/src/hooks.rs), [Bank storage worker](../patches/bank/src/storage_worker.rs), [Save and Quit steps](../patches/bank/src/save_steps.rs), [Bank navigation](../patches/bank/src/navigation.rs).
- [Transporter hooks](../patches/transporter/src/hooks.rs), [Transporter UI decisions](../patches/transporter/src/ui.rs), [Transporter storage replies](../patches/transporter/src/storage_worker.rs), [restore question and answer](../patches/transporter/src/restore.rs), [Transporter navigation](../patches/transporter/src/navigation.rs), [SD save writes](../patches/transporter/src/sdcard.rs).
- [Shared fault numbers](../crates/bank-common/src/fault.rs), [shared error-page composition](../crates/patch-common/src/numbers.rs), [Bank error dialog](../crates/bank-common/src/ui.rs), [delivery protocol](../crates/offline-core/src/transport.rs), [Bank guide](bank.md), [Transporter guide](transporter.md), [Internals](internals.md).
