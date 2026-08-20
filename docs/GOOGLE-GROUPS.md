# Google Groups mailboxes in Google Takeout

A Takeout archive contains **two different kinds of mbox**, not one. Besides the
familiar Gmail export, every group the account owns is exported as a full mbox
of its threads.

This document describes what those mailboxes look like and the exact rules
mboxShell applies to them, so the same behaviour can be reproduced in the macOS
and Windows apps. Implemented in mboxShell 0.7.0 — see
[issue #23](https://github.com/dcarrero/mboxshell/issues/23).

---

## 1. Where the files are

```
Takeout/Mail/All mail Including Spam and Trash.mbox    ← Gmail
Takeout/Groups/googlegroups.com/
  <groups-you-own>/<group>@googlegroups.com/topics.mbox    ← Google Groups
```

Every folder and file name is **localised to the account language**, including
the intermediate one shown as `<groups-you-own>` above. The Spanish export this
was built from reads:

```
Takeout/Grupos/googlegroups.com/
  grupos propios/medios-y-redes-general@googlegroups.com/temas.mbox
```

Two consequences:

* **Never key on the file name.** `topics.mbox`, `temas.mbox`, `Themen.mbox` …
  are all the same thing, and none of them identifies the group.
* **The parent directory is the identity.** It is always the group's posting
  address, `<group>@googlegroups.com`, in every locale.

These mailboxes are not small. In the archive this feature was built against,
the Groups mbox was the single largest file of the whole export — **636 MB /
6,787 messages**, larger than the Gmail mbox itself (292 MB).

Alongside `temas.mbox` the directory holds `información.csv` (group settings)
and `miembros.csv` (member list). Point mboxShell at the `.mbox` file itself —
opening the directory is not supported.

---

## 2. Groups-specific headers

```
From 28668125511680@xxx Thu Apr 16 09:53:04 +0000 2015
X-GM-THRID: 28616527183872
X-Google-Groups: medios-y-redes-general
X-Google-Thread: 428920,10b56905889cf582
X-Google-Attributes: gid428920,domainid0,private,googlegroup
X-Google-Language: SPANISH,ASCII
X-BeenThere: medios-y-redes-general@googlegroups.com
```

Coverage measured over the 6,787-message reference mailbox:

| Header | Present on | Notes |
|--------|-----------|-------|
| `X-GM-THRID` | 6,787 / 6,787 (100%) | 1,963 distinct values |
| `X-Google-Groups` | 6,785 / 6,787 | Bare group name, no domain |
| `X-BeenThere` | 4,029 / 6,787 | Full posting address |
| `Date:` | 6,787 / 6,787 (100%) | So the envelope date is never needed here |
| `X-Gmail-Labels` | 6 / 6,787 | Not Gmail labels — topic *state* (see below) |

`X-Gmail-Labels` deserves a note: it is present, but it does not carry Gmail
labels. Google reuses the header to record **topic state**, localised like
everything else and RFC 2047-encoded where needed:

```
X-Gmail-Labels: El tema se ha fijado
X-Gmail-Labels: =?UTF-8?Q?Las_respuestas_del_tema_est=C3=A1n_bloqueadas?=
```

Six messages out of 6,787 carry one. They are worth keeping — they are the only
record that a topic was pinned or locked — which is why the group label is
*added* to whatever the header holds instead of replacing it.

Line endings are CRLF, and not consistently: the same header appears with and
without a trailing `\r` across messages in one file. **Trim the value** or the
same group will show up as two distinct labels.

---

## 3. Rules mboxShell applies

### 3.1 The `From_` envelope date

Google Groups writes the envelope line with **the timezone offset before the
year**, which plain asctime does not allow:

```
From 28668125511680@xxx Thu Apr 16 09:53:04 +0000 2015
                                            ^^^^^ ^^^^
```

Format string: `%b %d %H:%M:%S %z %Y`, tried **after** plain asctime
(`%b %d %H:%M:%S %Y`), which it does not shadow.

This matters less than it looks: the envelope date is only a fallback for
messages with no parseable `Date:` header, and Groups messages always have one.
It is still worth handling, because the old failure mode was the unpleasant
kind — no error, just a plausible-looking **wrong** date (`2000-09-16`) that
quietly corrupted sort order and date filters.

### 3.2 The envelope sender is not an address

```
From 28668125511680@xxx ...
```

The address slot holds a numeric thread id at a bogus domain. Anything that
surfaces the envelope sender will show noise — always prefer the `From:`
header.

### 3.3 Group as a virtual label

A label sidebar built only on `X-Gmail-Labels` is all but empty for these
mailboxes — 6 messages out of 6,787, and what it holds is topic state rather
than a label anyone filters by. mboxShell derives a virtual label as well:

1. `X-Google-Groups`, trimmed, if non-empty → use as-is (`medios-y-redes-general`).
2. Otherwise `X-BeenThere`, **only if the domain is `googlegroups.com`** → use
   the local part.
3. Otherwise no label.

The domain check on `X-BeenThere` is deliberate: it is a generic mailing-list
header that Mailman writes too, and turning every mailing list into a label
would be a behaviour change of its own.

The label is *appended* to any `X-Gmail-Labels` already found rather than
replacing them, so a merged archive holding both Gmail and several groups can
be filtered by either.

### 3.4 Mailbox display name

A mailbox whose **parent directory** ends in `@googlegroups.com` (with a
non-empty local part) is named after that directory:

```
…/mi-grupo@googlegroups.com/temas.mbox   →   mi-grupo@googlegroups.com
```

This is the same class of problem as Apple Mail's `Inbox.mbox/mbox` package,
and it is handled in the same place. The resolved name is what the TUI shows
and what a merge writes into `X-Mbox-Source`.

### 3.5 Threading by `X-GM-THRID`

Groups (and Gmail) exports carry an explicit server-assigned conversation id.
mboxShell keeps the JWZ reference tree exactly as before, and uses the id only
where JWZ has to guess: the final step that merges root containers, which
otherwise merges by normalized subject.

* Roots **with** an id merge by id.
* Roots **without** one keep the subject heuristic.

Subject merging both over-merges (every conversation titled "Hello" becomes
one thread) and under-merges (a reply whose subject was edited splits off).
On the reference mailbox the change moved 1,927 → **1,958 threads**: 31
conversations that had been fused together are now kept apart, and the largest
thread shrank from 73 to 66 messages by shedding 7 unrelated ones.

Threading cost on that mailbox is ~15 ms for 6,787 messages, so the id path is
also cheaper than reconstructing chains.

---

## 4. Porting checklist

For the macOS and Windows apps:

- [ ] Accept a Groups mailbox from any locale — match on the parent directory
      (`*@googlegroups.com`), never on `topics.mbox` / `temas.mbox`.
- [ ] Parse `%b %d %H:%M:%S %z %Y` in the `From_` line, after asctime.
- [ ] Never trust the envelope sender; use `From:`.
- [ ] Trim header values before comparing (mixed CRLF).
- [ ] Derive the label: `X-Google-Groups`, else `X-BeenThere` restricted to
      `googlegroups.com`. Add it to `X-Gmail-Labels` rather than replacing —
      in a Groups mailbox that header holds topic state worth keeping.
- [ ] Name the mailbox after the group directory, and use that name wherever
      the source mailbox is recorded.
- [ ] Prefer `X-GM-THRID` over subject merging when grouping conversations.
- [ ] File pickers must let the user reach `temas.mbox` inside the Takeout tree
      (see issue #19 for a picker that greyed out non-`.mbox` names).

---

## 5. Test fixture

[`tests/fixtures/google_groups.mbox`](../tests/fixtures/google_groups.mbox)
holds three messages covering the cases above: one with no `Date:` header at
all, a reply sharing its `X-GM-THRID`, and a third with the same subject but a
different id (which must *not* be merged into the first thread). The assertions
live in [`tests/google_groups_tests.rs`](../tests/google_groups_tests.rs).

---

## License

mboxShell is released under the MIT License.
Copyright © David Carrero Fernández-Baillo — [carrero.es](https://carrero.es)
Source: [github.com/dcarrero/mboxshell](https://github.com/dcarrero/mboxshell)
