# Things

A personal desktop journal for keeping track of things, starting with games.
See [docs/pitch.md](docs/pitch.md) for the why and the design decisions.

![An entity page](docs/screenshots/entity-page.png)

## Run it

```sh
cargo run -- examples/data      # try it on the sample data
cargo run                       # uses $THINGS_DIR, else ~/things
```

The data folder is created if it doesn't exist. Ctrl+S (Cmd+S on macOS)
saves every changed component; switching to another entity or closing the
window saves too.

On Linux, eframe needs the usual windowing libraries at runtime
(`libxkbcommon-x11`, `libGL`/`libEGL`, and Wayland or X11).

## Layout

- `crates/things-core`: entity IDs, the file store, and the data formats of
  the components. No UI.
- `crates/things-app`: the egui app. `src/components/` has one module per
  component kind, each with its view and form.

## Data on disk

```
things/
  entities/
    mass-effect-7f3a/        readable slug + short ID
      status.ron             Status(state: Playing)
      moods.ron              Tags(tags: ["epic", "story-heavy"])
      gameplay.ron           Tags(tags: ["rpg", "single-player"])
      notes.md
      journal/
        2026-10-13-1930.md   one file per entry
```

Files are the source of truth. Any file the app doesn't recognise still
shows up on the entity page as plain text, so nothing is hidden.

## Adding a component

Write a type implementing `Kind` (which files it claims, how to open and
create them) and an `Editor` (its view and form) in
`crates/things-app/src/components/`, then add it to `registry()`. If its
data format is worth testing on its own, put the type in
`things_core::components`.
