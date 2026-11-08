# Pokémon Home

Whether Pokémon from the offline Bank can still reach Pokémon Home or the Switch games after the official Bank closes. The patches do not do this, and nothing here is planned.

**In short:** getting Pokémon onto a Switch without the servers is technically possible. Getting Home to accept them is not something any project can promise, as Home decides that on its servers, by rules it can change at any time.

## The official route ends with Bank

Pokémon Bank ends service on February 25, 2027, at 7:00 p.m. PST (February 26, 04:00 CET), and transfers from Bank to Home end with it ([Nintendo](https://en-americas-support.nintendo.com/app/answers/detail/a_id/61543/)).

The only official route for a Pokémon from a 3DS game to a Switch game is **3DS game → Pokémon Bank → Pokémon Home → Switch game** ([Pokémon Support](https://support.pokemon.com/hc/en-us/articles/360038131072-About-connecting-Pok%C3%A9mon-HOME-to-different-games)). No other legitimate way moves the same Pokémon onto the Switch with its 3DS origin kept.

## The Home tracker

When a Pokémon enters Home, the server gives it a **tracker**: a number that stands for a record on Home's servers. Trackers existed from Home's launch in February 2020, so there never was a time when Pokémon went from Bank to the Switch without one.

A tracker cannot be made up. A random number, or one copied from another Pokémon, has no matching record on the server. [PKHeX](https://github.com/kwsch/PKHeX/blob/3767c1367aeee2db93c2023d9938e9334e9d9819/PKHeX.Core/Legality/Verifiers/TransferVerifier.cs#L193) can tell that a required tracker is missing, but it cannot check a tracker that is there.

Three things therefore have to be kept apart, and one does not prove another:

| Statement | Who decides |
| --- | --- |
| The Pokémon's data passes a legality checker. | A community tool, from the data alone |
| Home accepts the Pokémon now. | Home's servers, by today's rules |
| The Pokémon came the official way. | Only Home's records |

## The Sword/Shield loophole

According to [Kaphotics](https://x.com/Kaphotics) (PKHeX's developer), a Pokémon of an older origin that is put into a Sword/Shield save can currently enter Home from there and is given a tracker. He also says that Home can tell on its servers that it came in through Sword/Shield and not through Bank. A genuine tracker therefore does not hide the route it skipped ([Project Pokémon](https://projectpokemon.org/home/forums/topic/56296-read-home-tracker-value/)).

What could follow, none of it confirmed or predictable:

- Home refuses new Pokémon that come this way.
- Home later restricts Pokémon it has already taken.
- Measures against the account or its access to the service.
- Users rely on a way of keeping their Pokémon whose rules are set elsewhere, and nothing promises that it stays open after Bank closes.
- A ready-made way from this project would draw attention to the loophole and make it far more likely to be closed, for everyone who uses it.

Closing it looks easy: Home could require a valid tracker for every origin whose official way led through Bank. That is a conclusion from the transfer rules, not a sign that it is planned.

## Ways onto a Switch

Assuming Home keeps accepting Pokémon from Sword/Shield that it has never seen, a Pokémon from the offline Bank could get there in several ways. Each needs it converted correctly first: species, forms, moves and the other rules of the game still apply.

| Way | How | Main limit |
| --- | --- | --- |
| LAN trade | Convert the Pokémon and act as a trading partner over an ordinary local network. | One Pokémon per trade, each confirmed. |
| Local wireless trade | Act as a trading partner over the Switch's own wireless, with a computer and an ESP32 board as its radio. | Needs the board and the computer. |
| Save import | Put the converted Pokémon into a Sword/Shield save. | Needs a modified Switch to reach the save. |
| Local Mystery Gift | Send Wonder Cards that Sword/Shield receives. | A card cannot carry a whole existing Pokémon ([below](#wonder-cards)). |

**LAN trading** is the most promising. Sword/Shield has a hidden LAN mode that runs over the console's ordinary network, and pokeldn completed trades with a retail Sword on a Switch 2 over Wi-Fi, both joining and hosting ([pokeldn: LAN mode](https://decryptu.github.io/pokeldn/swsh_session.html#lan-mode)). A client on the 3DS would still have to be written and tested.

**Local wireless** has a published implementation as well. The protocol runs on a computer, and the ESP32 board is only its radio ([pokeldn: hardware](https://decryptu.github.io/pokeldn/hardware.html)).

## Wonder Cards

A Sword/Shield Wonder Card (WC8) is a 720-byte **template for one gift Pokémon**, not a complete record of an existing one. It sets many fields but not every one a Pokémon carries, and it has room for 32 ribbons. PKHeX's conversion of a card into a Pokémon marks it as a fateful encounter, so an ordinary caught Pokémon made from a card could possibly change how it was met.

From pokeldn's research ([pokeldn: Mystery Gift](https://decryptu.github.io/pokeldn/swsh_gift.html)):

- A host computer delivers cards to a retail Sword over local wireless. From the ESP32 board alone this is not measured.
- One message can hold several cards: the receiver takes any whole number of 720-byte records. How many are redeemed in a row is not shown.
- A card can be limited to once per card id, or to once per card date and ten a day; others can be received any number of times.
- The album keeps the newest 50 cards. That is a list, not a limit on how many cards a save can ever receive.
- A Pokémon card needs a free place in the party or the boxes.

A card the game accepts is still not an encounter that could be had officially, and it cannot give the Pokémon a Home tracker.

## Sending from a 3DS

The 3DS talks to other consoles locally through UDS. Sword/Shield's local gifts come as encrypted Switch LDN advertisements ([pokeldn: LDN](https://decryptu.github.io/pokeldn/ldn.html)). Sending a card's bytes through the 3DS's ordinary wireless functions does not produce such an advertisement. No 3DS that sends one on its own was found, which does not prove that its hardware cannot.

A LAN trade avoids the radio entirely, since both consoles talk through an ordinary router.

## For this project

Sending Pokémon to a Switch locally could be an experiment for keeping them, not a promise of Home support, unless Home itself comes to allow such transfers. Changing a Pokémon's origin to get it accepted would misstate its history, and a tracker obtained through today's loophole guarantees nothing for tomorrow.
