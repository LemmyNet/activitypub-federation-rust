<!-- be sure to keep this file in sync with README.md -->

A high-level framework for [ActivityPub](https://www.w3.org/TR/activitypub/) federation in Rust. The goal is to encapsulate all basic functionality, so that developers can easily use the protocol without any prior knowledge.

The ActivityPub protocol is a decentralized social networking protocol. It allows web servers to exchange data using JSON over HTTP. Data can be fetched on demand, and also delivered directly to inboxes for live updates.

While Activitypub is not in widespread use yet, is has the potential to form the basis of the next generation of social media. This is because it has a number of major advantages compared to existing platforms and alternative technologies:

- **Interoperability**: Imagine being able to comment under a Youtube video directly from twitter.com, and having the comment shown under the video on youtube.com. Or following a Subreddit from Facebook. Such functionality is already available on the equivalent Fediverse platforms, thanks to common usage of Activitypub.
- **Ease of use**: From a user perspective, decentralized social media works almost identically to existing websites: a website with email and password based login. Unlike pure peer-to-peer networks, it is not necessary to handle private keys or install any local software.
- **Open ecosystem**: All existing Fediverse software is open source, and there are no legal or bureaucratic requirements to start federating. That means anyone can create or fork federated software. In this way different software platforms can exist in the same network according to the preferences of different user groups. It is not necessary to target the lowest common denominator as with corporate social media.
- **Censorship resistance**: Current social media platforms are under the control of a few corporations and are actively being censored as revealed by the [Twitter Files](https://jordansather.substack.com/p/running-list-of-all-twitter-files). This would be much more difficult on a federated network, as it would require the cooperation of every single instance administrator. Additionally, users who are affected by censorship can create their own websites and stay connected with the network.
- **Low barrier to entry**: All it takes to host a federated website are a small server, a domain and a TLS certificate. All of this is easily in the reach of individual hobbyists. There is also some technical knowledge needed, but this can be avoided with managed hosting platforms.

Below you can find a complete guide that explains how to create a federated project from scratch.

Feel free to open an issue if you have any questions regarding this crate. You can also join the Matrix channel [#activitystreams](https://matrix.to/#/%23activitystreams:matrix.asonix.dog) for discussion about Activitypub in Rust. Additionally check out [Socialhub forum](https://socialhub.activitypub.rocks/) for general ActivityPub development.