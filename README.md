Activitypub-Federation
===
[![Build Status](https://drone.join-lemmy.org/api/badges/LemmyNet/activitypub-federation-rust/status.svg)](https://drone.join-lemmy.org/LemmyNet/activitypub-federation-rust)
[![Crates.io](https://img.shields.io/crates/v/activitypub-federation.svg)](https://crates.io/crates/activitypub-federation)

A high-level framework for [ActivityPub](https://www.w3.org/TR/activitypub/) federation in Rust, extracted from [Lemmy](https://join-lemmy.org/). The goal is that this library can take care of almost everything related to federation for different projects. For now it is still far away from that goal, and has many rough edges that need to be smoothed.

You can join the Matrix channel [#activitystreams:matrix.asonix.dog](https://matrix.to/#/%23activitystreams:matrix.asonix.dog?via=matrix.asonix.dog) to discuss about Activitypub in Rust.

## Features

- ObjectId type, wraps the `id` url and allows for type safe fetching of objects, both from database and HTTP
- Queue for activity sending, handles HTTP signatures, retry with exponential backoff, all in background workers
- Inbox for receiving activities, verifies HTTP signatures, performs other basic checks and helps with routing
- Data structures for federation are defined by the user, not the library. This gives you maximal flexibility, and lets you accept only messages which your code can handle. Others are rejected automatically during deserialization.
- Generic error type (unfortunately this was necessary)
- various helpers for verification, (de)serialization, context etc

## How to use


To get started, have a look at the [API documentation](https://docs.rs/activitypub_federation/latest/activitypub_federation/)
and [example code](https://github.com/LemmyNet/activitypub-federation-rust/tree/main/examples). You can also find some [ActivityPub resources in the Lemmy documentation](https://join-lemmy.org/docs/en/contributing/resources.html#activitypub-resources).
If anything is unclear, please open an issue for clarification. For a more advanced implementation,
take a look at the [Lemmy federation code](https://github.com/LemmyNet/lemmy/tree/main/crates/apub).

Currently supported frameworks include [actix](https://actix.rs/) and [axum](https://github.com/tokio-rs/axum):

**actix:**

```toml
activitypub_federation = { version = "*", features = ["actix"] }
```

**axum:**

```toml
activitypub_federation = { version = "*", features = ["axum"] }
```

## Roadmap

Things to work on in the future:
- **Improve documentation and example**: Some things could probably be documented better. The example code should be simplified. where possible.
- **Simplify generics**: The library uses a lot of generic parameters, where clauses and associated types. It should be possible to simplify them.
- **Improve macro**: The macro is implemented very badly and doesn't have any error handling.
- **Generate HTTP endpoints**: It would be possible to generate HTTP endpoints automatically for each actor.
- **Support for other web frameworks**: Can be implemented using feature flags if other projects require it.
- **Signed fetch**: JSON can only be fetched by authenticated actors, which means that fetches from blocked instances can also be blocked. In combination with the previous point, this could be handled entirely in the library.
- **Helpers for testing**: Lemmy has a pretty useful test suite which (de)serializes json from other projects, to ensure that federation remains compatible. Helpers for this could be added to the library.
- **[Webfinger](https://datatracker.ietf.org/doc/html/rfc7033) support**: Not part of the Activitypub standard, but often used together for user discovery.
- **Remove request_counter from API**: It should be handled internally and not exposed. Maybe as part of `Data` struct.
- 
## License

Licensed under [AGPLv3](LICENSE).
