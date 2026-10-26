# Changelog

## [0.3.0](https://github.com/vanducng/rexboard/compare/v0.2.0...v0.3.0) (2026-10-26)


### ⚠ BREAKING CHANGES

* **window:** `rexboard window` and `jump.pre_cmd` are removed, along with the `JUMP` error code that only pre_cmd raised. Run `rexboard` in the Rex tab you want the board in; a jump opens the agent's session in that same window. An old `[jump]` table is ignored.

### Features

* **tui:** a table view sorted by any column, by key or header click ([495b3c5](https://github.com/vanducng/rexboard/commit/495b3c564cfa516fb4f9cc0e79ade09162525f28))
* **tui:** fill the detail pane with the peek's newest lines, wrapped from the bottom ([#24](https://github.com/vanducng/rexboard/issues/24)) ([a3b24f7](https://github.com/vanducng/rexboard/commit/a3b24f7fbbc34e57caba3cc7fb77a56beb50aa19))
* **tui:** filter the board to one status with f and a letter ([#20](https://github.com/vanducng/rexboard/issues/20)) ([e50bdc1](https://github.com/vanducng/rexboard/commit/e50bdc1d2ad87e4fa40c2ac5e0bc179f9bad8b28))
* **tui:** put a blank row between the board's title and its column header ([#17](https://github.com/vanducng/rexboard/issues/17)) ([2457d0b](https://github.com/vanducng/rexboard/commit/2457d0b17ff74f0714b5dc3571934c317f12f2db))
* **tui:** split WHERE into SESSION, WINDOW and BLOCK on a wide list ([#26](https://github.com/vanducng/rexboard/issues/26)) ([5fc01a1](https://github.com/vanducng/rexboard/commit/5fc01a12bbd8a036db02dd8eb52c51fb75ca201f))
* **tui:** widen the detail pane to a third and let &lt; and &gt; resize it ([#21](https://github.com/vanducng/rexboard/issues/21)) ([2b09a32](https://github.com/vanducng/rexboard/commit/2b09a32cdc98bc1f2025e98e992a62a2837a432c))


### Bug Fixes

* **serve:** put the socket beside serve.lock so every caller finds the one serve ([#19](https://github.com/vanducng/rexboard/issues/19)) ([bc82864](https://github.com/vanducng/rexboard/commit/bc828644afe3e1ebeea0f2d0d0ce961441b1224c))
* **tui:** stop table-view redraws on mouse motion, keep a clicked row in place, and stop the wheel at the ends ([#29](https://github.com/vanducng/rexboard/issues/29)) ([4769a60](https://github.com/vanducng/rexboard/commit/4769a6018e73fb06b69878ecdfd35cd52ec39cea))


### Code Refactoring

* **window:** remove the window command, jump.pre_cmd and the board's terminal title ([#25](https://github.com/vanducng/rexboard/issues/25)) ([d1e76cc](https://github.com/vanducng/rexboard/commit/d1e76ccd420f5dcb4754f61dac5a37d6f3638a5a))

## [0.2.0](https://github.com/vanducng/rexboard/compare/v0.1.0...v0.2.0) (2026-10-25)


### Features

* **tui:** name the board's columns in a header row ([fb944ec](https://github.com/vanducng/rexboard/commit/fb944ec36aef40d19494618b9c84f1679a368619))
* **tui:** views by status, flat, harness and host, with k9s-style sort ([82f8112](https://github.com/vanducng/rexboard/commit/82f811237ef3c9ae9d972f0699ec7ffd88bebb42))
* **window:** show the board in the Rex window you are in ([9049b16](https://github.com/vanducng/rexboard/commit/9049b165ebe92ad080c63ad97a1de56b8a050461))


### Bug Fixes

* **event:** keep a host alive when Rex sends an observer line rexboard cannot read ([#15](https://github.com/vanducng/rexboard/issues/15)) ([9a23779](https://github.com/vanducng/rexboard/commit/9a23779ed3578193b64b2f5fa56812d5fd81dcea))

## 0.1.0 (2026-10-25)


### Features

* **board:** derive attention rows with since, turn, seen and groups ([#5](https://github.com/vanducng/rexboard/issues/5)) ([56aa7fc](https://github.com/vanducng/rexboard/commit/56aa7fc5244d56503f168326016ebbbbd8b49546))
* **jump:** jump, next and pick land on the pane in the main Rex window ([#8](https://github.com/vanducng/rexboard/issues/8)) ([35deff4](https://github.com/vanducng/rexboard/commit/35deff491a415ea3e68799abf1c16a6e328fad7c))
* **ls:** list agent panes across Rex hosts from one Lua snapshot ([#2](https://github.com/vanducng/rexboard/issues/2)) ([b6859f0](https://github.com/vanducng/rexboard/commit/b6859f0a38bbe92ddea5043bbb84e6b1d688d09f))
* **serve:** share board state through a daemon with persisted since and seen ([#6](https://github.com/vanducng/rexboard/issues/6)) ([1d910e4](https://github.com/vanducng/rexboard/commit/1d910e4316b1ec6f0c1bbce22bb6abb09e487343))
* **tui:** board TUI with grouped attention rows, detail and peek ([#7](https://github.com/vanducng/rexboard/issues/7)) ([d8c173e](https://github.com/vanducng/rexboard/commit/d8c173e0d3f70b6543d812e344c3a5adad26c675))
* **watch:** stream host events from one Lua observer per host ([#3](https://github.com/vanducng/rexboard/issues/3)) ([91f5d24](https://github.com/vanducng/rexboard/commit/91f5d247ba223969c72f7080782406a07a2a1e02))
* **window:** show the board in its own Rex app window, with the README demo ([#9](https://github.com/vanducng/rexboard/issues/9)) ([7348ce3](https://github.com/vanducng/rexboard/commit/7348ce3ee8b94eddcdc87f03011549d0285997c5))


### Bug Fixes

* **rex:** keep caller targets out of rex calls, attach before destroy ([#10](https://github.com/vanducng/rexboard/issues/10)) ([55e2a22](https://github.com/vanducng/rexboard/commit/55e2a222934a63211aaade6044a4f2f61a16de41))
