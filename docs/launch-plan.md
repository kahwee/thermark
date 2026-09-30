# Adoption plan

Reviewed 2026-09-25. This is a maintainer plan, not a claim that outreach below
has been submitted or that any community will feature the project.

## Current distribution

- [crates.io 0.33.0](https://crates.io/crates/thermark/0.33.0) and
  [API documentation](https://docs.rs/thermark/0.33.0/thermark/) are live.
- [GitHub release](https://github.com/kahwee/thermark/releases/tag/v0.33.0)
  supplies BLE/full archives for macOS and Linux, ARM64 and X64.
- [Homebrew tap](https://github.com/kahwee/homebrew-thermark) supplies macOS BLE.
- [Landing page](https://kahwee.github.io/thermark/) has offline rendered demos.
- [X announcement](https://x.com/kahwee/status/2103680483083350258) is published.
- [B1 tester issue](https://github.com/kahwee/thermark/issues/1) already exists.

## First milestone: five independent B1 reports

Prioritize successful installation and one useful label over download or star
counts. Invite B1 owners on macOS and Linux to render the example, connect, print
one URL QR label, and scan it. Record installation method, OS/CPU, whether each
step worked, and the blocker when it did not. Ask willing testers to repeat a
print and report consistency. Accept failed installs as useful evidence.

Use the existing issue and hardware-report form; avoid splitting a small user
base across new Discord, Slack, and Discussion channels. Review incoming
reports manually and fix the most common setup failure before broader outreach.
Only update the profile registry's verification evidence after actual hardware
results. No telemetry is required for this milestone.

## Where to share next

| Priority | Place | Suitable contribution and prerequisite |
| --- | --- | --- |
| 1 | Existing X audience and B1 owners you already know | Follow up with a real print video and one clear request: try one label and report the outcome. Avoid repeating the same announcement. |
| 2 | r/rust weekly “What's Everyone Working on This Week?” | A personally written account of the work and a link. The current moderator announcement directs casual project sharing here; do not submit a bare repository launch as a new post. |
| 3 | This Week in Rust, Call for Participation | Propose a specific open contributor task with difficulty, requirements, and contribution-guide link. A hardware-required task should say so explicitly; editors decide suitability. |
| 4 | Show HN | After independent setup reports and real footage, personally describe the problem, working software, limitations, and lessons. Be available to answer questions. |
| Later | Local maker groups and printer communities | Check each group's current rules and ask moderators where needed. Share a relevant workflow, not repeated link drops. r/niimbot was restricted during the launch review. |

Current source rules:

- [r/rust moderator announcement](https://www.reddit.com/r/rust/comments/1wkmzun/no_more_code_dumps/)
  directs casual sharing to its weekly thread and prohibits generated articles
  and posts. The maintainer must write their contribution personally.
- [This Week in Rust contribution rules](https://github.com/rust-lang/this-week-in-rust/blob/main/README.md)
  no longer accept PR submissions for Project/Tooling Updates. Call for
  Participation remains a separate route with specific issue requirements.
- [HN guidelines](https://news.ycombinator.com/newsguidelines.html) prohibit
  generated or AI-edited text; [Show HN guidelines](https://news.ycombinator.com/showhn.html)
  favor something people can try. Do not copy AI-written launch text there.

## Missing assets and next actions

1. Update the existing tester issue's old 0.32.0 download link, add Homebrew and
   Cargo options, an explicit `--model b1` to its offline preview, and the current
   contribution guide. Keep the same issue so existing links and replies survive.
2. Record a 20–30 second real B1 print: show the command, paper emerging, finished
   label, and a phone scanning example.com. Use demo data and conceal printer
   identifiers. The website animation is not physical footage.
3. Recruit the first five testers, including at least one Linux user. Get their
   permission before quoting or reposting their photos. Fix reproducible
   installation problems and document confirmed platform limitations.
4. Submit one suitable community contribution at a time, using the routes above.
   Answer responses before moving to the next audience. No paid ads are needed
   to establish whether first-time users can print successfully.
5. Review results after two weeks: independent attempts, successful first prints,
   repeat-use reports, and resolved setup blockers. GitHub traffic and crate
   downloads are supporting signals, not proof of active users.

## Next release hygiene

- The 0.33.0 registry package and archives contain the README as it stood before
  publication, including the then-pending crates.io note. Keep release artifacts
  immutable; ship current documentation with the next intentional release.
- Keep Cargo version, tag, GitHub assets, Homebrew version/checksums, release notes,
  and install documentation aligned. Verify an installed artifact's offline
  preview as well as its `--version` output.
- Consider crates.io trusted publishing for future releases to reduce manual
  token handling. Review repository/environment permissions before enabling it;
  it is not configured by this plan.
- Keep generated proof, personal printer data, and local work under ignored
  `local/`. Do not remove credentials, configuration, or local work as cleanup.

## Recipe-led promotion: next two weeks

Prepared 2026-09-30. These are proposed actions; no outreach has been sent.

Lead with a useful result: “Put a scannable manual on your tools” or “Label
storage bins from your terminal.” Link each demonstration to its specific
[recipe](recipes.md), so the reader can recreate the result immediately.
Keep “Print labels without the vendor app” as the common project description.

| When | Action | Evidence to collect |
| --- | --- | --- |
| Days 1–2 | Publish the recipe gallery with the normal documentation workflow; update the existing tester issue. | All eight commands render; installation links work. |
| Days 3–4 | Record one real B1 print and QR scan using the equipment-manual recipe. Show installation method and label size in the accompanying explanation. | Actual paper output and successful scan; no printer identifiers or private data. |
| Days 5–7 | Share that workflow with the existing audience and invite B1 owners to try one recipe. Link the existing tester issue. | Independent first-print attempts, OS, install method, and blockers. |
| Week 2 | Show a second workflow based on tester feedback, then choose one community route from the table above. | Repeat use and resolved setup problems; answer questions before another submission. |

For a Show HN, link runnable software and make the offline recipes easy to find:
[the guidelines](https://news.ycombinator.com/showhn.html) ask for something
people can try, including software or a hardware video. The animation alone
is not evidence of a physical print. Write the community submission personally
and check the destination's current rules before posting.

For This Week in Rust, a scoped contributor issue remains a better fit than a
version announcement. Its [contribution guide](https://github.com/rust-lang/this-week-in-rust/blob/main/README.md)
accepts calls for participation but no longer accepts PR submissions to the
Project/Tooling Updates section. Choose a real task with clear acceptance
criteria, such as documenting a reproducible Linux installation failure or
reporting a physical print on an experimental profile; state hardware needs.

Expand examples when someone supplies a real workflow. Require public demo
data, a copyable offline command, a preview generated by thermark, and a short
explanation of what to replace. Add it to `examples/render-recipes.sh` and the
recipe guide together. Keep physical verification claims separate from renders.

Track five independent first-print reports, at least one Linux attempt, and two
reports of repeat use. These are targets, not observed results. After two weeks,
use the reports to decide whether installation, examples, or printer support
needs the next investment. Stars alone do not answer that question.
