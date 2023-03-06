## Federating posts

We repeat the same steps taken above for users in order to federate our posts.

```text
$ curl -H 'Accept: application/activity+json' https://mastodon.social/@LemmyDev/109790106847504642 | jq
{
    "id": "https://mastodon.social/users/LemmyDev/statuses/109790106847504642",
    "type": "Note",
    "content": "<p><a href=\"https://mastodon.social/tags/lemmy\" ...",
    "attributedTo": "https://mastodon.social/users/LemmyDev",
    "to": [
        "https://www.w3.org/ns/activitystreams#Public"
    ],
    "cc": [
        "https://mastodon.social/users/LemmyDev/followers"
    ],
}
```

The most important fields are:
- `id`: Unique identifier for this object. At the same time it is the URL where we can fetch the object from
- `type`: The type of this object
- `content`: Post text in HTML format
- `attributedTo`: ID of the user who created this post
- `to`, `cc`: Who the object is for. The special "public" URL indicates that everyone can view it.  It also gets delivered to followers of the LemmyDev account.

Just like for `Person` before, we need to implement a protocol type and a database type, then implement trait `ApubObject`. See the example for details.
