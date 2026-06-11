# Tubbie 1.2

A lighter, faster, sharper-looking release. The download is smaller, the app
reaches your first board quicker, and the settings and status screens read more
crisply — with the dot-matrix board itself left exactly as crisp as before.

## Smaller download

The release build is about 38% smaller — roughly 18 MB down to 11 MB. A
size-tuned build profile (link-time optimization, stripped symbols) and pruning
nine dependencies that were declared but never actually used got the binary down
without dropping a single feature.

## Faster startup

The three settings the app reads on launch — display mode, display preferences,
and your saved config — now load in parallel instead of one after another. That
trims a couple of round-trips of dead time before your first board appears.

## Sharper text on the chrome screens

Settings, the status view, and the first-run prompt now render with font
smoothing on, so body text reads cleanly. The arrivals board, ticker, and
platform columns are untouched — they keep their deliberately pixel-crisp
dot-matrix faces.

---

**Install:** download the `.dmg`, drag Tubbie to Applications, and launch. The
build is signed with a Developer ID and notarized by Apple. Existing installs
update in-app.
