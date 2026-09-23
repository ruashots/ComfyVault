# Screenshots the README is waiting for

The README ships with no pictures. This file records the four that were designed
into it, so that whoever takes them knows what to capture and where each one
goes back.

Nothing in the README refers to these files today, so the page is correct
without them. Adding one means putting the file in this folder and adding its
line to `README.md` at the place named below.

---

## The rules that apply to all four

**Capture the real application, never the design mock.** The mock at
`design/mock/comfyvault.html` is a drawing. It is not the product, and a drawing
presented as a screenshot is the fastest way to lose a reader's trust.

**Use real installs and real model names.** No `foo`, no `test1.safetensors`, no
lorem. A stranger can tell the difference in under a second, and invented content
reads as a product nobody has used.

**Never show a path that belongs to someone else's private machine** beyond the
install folders themselves. Model names are fine. A personal folder name is not.

**Capture the whole window, including its own title bar.** ComfyVault draws its
own title bar, and it carries the vault path, which tells the reader at a glance
that the app knows where the vault is.

**The window is 1000 by 660.** Capture at that size, or larger on a high density
display, and do not scale it down before committing it. Do not crop to a single
panel: the left rail with the drive meter is part of what every one of these
pictures is saying.

**Keep the four consistent.** Same window size, same theme, same installs, same
session if you can. Four pictures of four different states of four different
machines reads as a mock-up.

**Nothing in the window can be mid-animation.** No toast fading, no spinner, no
half-drawn progress bar, unless the picture is specifically of work in progress.

---

## 1. `home.png`

**Goes:** in `README.md`, directly under the opening paragraph, before the
`**Before.**` line. It is the first thing a visitor sees.

**Caption:** `ComfyVault Home, after a scan of three ComfyUI installs`

**Screen:** Home, after a scan has finished. Not the first-run empty state, and
not the scanning state.

**State it must be in:** at least two installs registered, ideally three. A scan
completed, so the four tiles hold real numbers. A plan built, so the drive meter
in the left rail shows the amber band for space that can still be freed.

**What has to be visible:**

- The four tiles across the top: instances, unique models, models on disk, and
  the not-used count.
- The left rail's drive meter, with the amber band, and the line under it
  reading how much can be freed.
- The vault path in the title bar.

**What this picture has to say:** this is a real desktop application that has
already read this machine and knows what is on the drive. It is the only picture
that gets a chance to say it, so it is the one worth retaking until it is right.

**If the machine has nothing left to free**, that is the wrong moment to capture
it. The amber band carrying a real number is most of the value here.

---

## 2. `consolidate.png`

**Goes:** in `README.md`, at the end of the section `## How a consolidation
goes`, after step 5.

**Caption:** `The Consolidate screen: a dry run, before anything moves`

**Screen:** Consolidate, showing the plan.

**State it must be in:** a plan built from a scan that found real duplicates.
Scrolled so that at least one duplicate group is open, showing which copy is
kept and which paths become links.

**What has to be visible:**

- The subtitle under the title, reading that this is a dry run and nothing has
  moved. This is the whole point of the picture.
- The space returned figure.
- One expanded duplicate group with its real file paths, so the reader can see
  one copy marked as the one that moves and the others marked as links.

**What this picture has to say:** you read this before anything happens to your
files.

---

## 3. `apply-done.png`

**Goes:** in `README.md`, immediately after `consolidate.png`. The two are a
pair: the plan, then the result.

**Caption:** `The same run, finished: what moved, what was linked, and the button
that undoes it`

**Screen:** the finished run, which Consolidate shows after Apply.

**State it must be in:** a run that actually completed, ideally the same run as
the picture above, so the numbers in the two pictures agree. A reader who checks
will check that.

**What has to be visible:**

- The large reclaimed figure at the top, and the line reading how much was free
  before and how much is free now.
- The what happened rows: files moved, links created, groups asked for.
- The control that undoes the run.

**What this picture has to say:** it worked, here is exactly what it did, and it
can be taken back.

---

## 4. `cleanup.png`

**Goes:** in `README.md`, at the end of the section `## The screens`.

**Caption:** `Cleanup: broken links, duplicate names, and vault files nothing
points at`

**Screen:** Cleanup.

**State it must be in:** with real content in at least one of its three
sections. Best is all three: a link pointing at a file that is not there, a
model carrying two names, and a vault file nothing links to.

**What has to be visible:** whichever sections have content, with their real
counts in the header line.

**Do not manufacture a broken link to fill the picture.** If the machine has
none, capture the screen with the name groups and the unused vault files, and
leave the broken links section showing its empty state. An honest empty state is
worth more here than a staged failure, and the empty states were written to be
read.

---

## Lower priority, and not referenced by the README

Two more were considered and left out. Neither is needed. If either is taken,
the README has to gain a line for it.

**The blocked panel on Consolidate, with Windows Developer Mode off.** It shows
the application refusing to move anything and saying exactly why. It is the best
single picture of the product's caution, and it is easy to produce: turn
Developer Mode off and open Consolidate.

**The Library list.** One row per unique model, with the count of how many places
hold each one. It is the screen that shows the product understands a collection
rather than a folder.
