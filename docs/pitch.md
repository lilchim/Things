# Things: Project Pitch

Oct 13, 2026 · @Findle

**Things is a personal desktop journal for keeping track of things, starting with my games: what I've played, what I want to play, and what I was doing when I stopped.**

## The problem

I own thousands of games on Steam and I can't find one to play that fits my mood. I buy on sale, so the backlog keeps growing, and nothing tracks it. Steam's "recent" sort orders games by purchase date until playing reshuffles them. When I scroll the library I often don't remember what a game is or why I bought it, so I go back to its store page, and usually decide "not right now."

When I do come back to a game after eight months, I've forgotten where I was and what I was doing. And events in live-service games slip past because nothing reminds me.

## What Things is

Things is a desktop app with a real UI, written in Rust, built for one user: me. It is not commercial and not shared.

Games come first, but the same model holds anything worth logging: a work project, a Twitter post I want to keep, a calendar of what's coming up. Over time it becomes my diary of things, stored as plain files I can put in a Git repo.

## Core idea: entities on disk, components as files

The architecture is the point of the project. It is an entity-attribute-value (EAV) model, with components as the attributes, stored as files rather than in a database. It borrows ECS vocabulary but is not an ECS: there are no systems, apart from a few background services such as reminders.

- **An entity is just an identifier.** A game, a series, a person or a post is a folder named with a readable slug plus a short ID, such as `mass-effect-7f3a`. It has no type and no schema of its own.
- **A component is one file and owns its format.** Most components store RON, which maps straight onto Rust structs. A Markdown component is simply a document that belongs to the entity. HTML can come later. Each component also owns its UI: a standard view and a form for creating or editing it.
- **Components are reusable and pinned on freely.** Notes, journal entries, tags and reminders work on any entity. A game is simply an entity that happens to have a Steam component.
- **The UI is assembled at open time.** Opening an entity reads every component file in its folder and lays out each one's view, so the page reflects whatever that entity has.

A game entity on disk might look like this:

```
things/
  games/
    mass-effect-7f3a/
      steam.ron            app id, playtime, last played
      status.ron           backlog | playing | finished | shelved
      moods.ron            chill, epic, story-heavy
      gameplay.ron         rpg, single-player, third-person
      series.ron           link to the mass-effect-series entity
      notes.md             where I left off
      reminders.ron        upcoming events
      journal/
        2026-10-13-1930.md   one file per entry
        2026-06-02-2100.md
```

Because everything is plain text with a consistent naming convention, the data stays portable, diffable and easy to keep in Git.

## First version

The first version proves the entity and component model on my game backlog. Each component below is one feature.

| Component | What it gives me |
| --- | --- |
| Steam link | Imports my library once, so every game exists as an entity with its app ID and playtime |
| Play status | Backlog, playing, finished or shelved, so the backlog is finally tracked |
| Mood tags | How a game feels (chill, tense, epic), so I can filter for what fits my mood tonight |
| Gameplay tags | How a game plays (RPG, single-player, co-op), kept separate from mood |
| Notes | A Markdown document: why I bought it and where I left off |
| Journal entry | One Markdown file per dated entry, about a play session or anything else |
| Reminder | A date and a note, such as a live-service event, shown in an upcoming list on the home screen |

Mood and gameplay tags are two instances of one tagging component. More tag sets can be added later without new code, because each set covers its own concept and avoids one tag list that is too broad or too narrow.

Around those, the app needs a library view to browse and filter entities, an entity page that renders its components, and a way to add a component to any entity.

## Principles and non-goals

- **Files are the source of truth.** The app reads and writes plain files; any index or cache can be rebuilt from them.
- **Entities stay loose.** No schema. What something is comes from the components it has. Collections (games, tasks) keep different kinds apart and give new things their starting components, but never restrict what a thing can have.
- **One user, local first.** No accounts, no server, no sharing.
- **Grow by adding components.** A new feature is a new component with a data format and a view, not a change to the core.
- **Not now:** recommendations, sale tracking, LLM summaries and other discovery features. Those can come later.

## Relationship to sgnome

sgnome tackled the same core problem, finding a game to play in my own library, but for anyone: linked accounts, a node-based workflow to explore owned and unowned games, recommendations, sale conditions and templated LLM prompts. Things does not use or change sgnome. It is a reference for how I worked with the Steam API. Some sgnome features may fold into Things later, likely as new components.

## Decisions so far

| Topic | Decision |
| --- | --- |
| Pattern | Entity-attribute-value with file-backed components; no ECS |
| Data format | Each component owns its format: RON by default, Markdown for documents, HTML later |
| Journal | One file per entry |
| Entity names | Readable slug plus a short ID |
| Choosing by mood | Several tag sets built on one tagging component: mood, gameplay, more later |
| Links between entities | Two kinds: stored links on disk (a game belongs to a series) and in-memory indexes built from shared properties (every game tagged "chill") |
| UI toolkit | egui, an immediate-mode toolkit: the UI is redrawn from the data every frame, with no two-way data binding |
| Collections | Each thing lives in one collection folder (`games/`, `tasks/`) with a `collection.ron` naming its starting components and whether it feeds the radar. IDs stay unique across collections so links survive a move. Groupings that cut across collections stay with tags and search |

## Still open

- **How stored links are written.** One option: a series is its own entity, each game carries a small link component pointing to it, and the reverse view ("what else is in this series?") comes from the index. That also enables "you own Mass Effect 1 but not 2; add it?" The links don't have to be typed by hand: research tools can fill them in from game data APIs (IGDB, for one, tracks series and franchises) or from web searches.
- **Showing HTML such as a Steam store page (nice to have).** egui does not render HTML, so this needs an embedded webview in its own window, or opening the page in the browser.
- **Indexes at startup.** Rebuild them by scanning every file on launch, or cache them and refresh on change.
