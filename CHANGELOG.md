# Changelog

## [0.14.1](https://github.com/vanducng/rexboard/compare/v0.14.0...v0.14.1) (2026-10-27)


### Bug Fixes

* **hooks:** hooks and the status line fail open, by full path ([95ce655](https://github.com/vanducng/rexboard/commit/95ce6553af212b713820e969638407ceb21cf28d))

## [0.14.0](https://github.com/vanducng/rexboard/compare/v0.13.0...v0.14.0) (2026-10-27)


### Features

* **hooks:** rexboard hook and hooks install, status and replay, for Claude Code, Codex and pi ([3817e60](https://github.com/vanducng/rexboard/commit/3817e60ef9f5a0c33904e88b00207db9cf64f201))

## [0.13.0](https://github.com/vanducng/rexboard/compare/v0.12.0...v0.13.0) (2026-10-27)


### Features

* **harness:** model and context per agent, from each harness's own turn ([7f15ba5](https://github.com/vanducng/rexboard/commit/7f15ba54aaf459f356116d19d17dc2a53d162cc7))

## [0.12.0](https://github.com/vanducng/rexboard/compare/v0.11.1...v0.12.0) (2026-10-26)


### Features

* **cli:** answer and close a pane, pinned to the dialog the caller read ([c43955b](https://github.com/vanducng/rexboard/commit/c43955b6b1d0a8ba8c3eeb8460d42906fc985f7c))

## [0.11.1](https://github.com/vanducng/rexboard/compare/v0.11.0...v0.11.1) (2026-10-26)


### Bug Fixes

* **tui:** one card height, its text scrolling, a blank row above the footer ([293e3af](https://github.com/vanducng/rexboard/commit/293e3af4f4b3002ea02ee57e0a58c5175127db02))

## [0.11.0](https://github.com/vanducng/rexboard/compare/v0.10.0...v0.11.0) (2026-10-26)


### Features

* **cli:** show, views, hosts, and ls view, filter, status and sort, as on the board ([5c2658a](https://github.com/vanducng/rexboard/commit/5c2658a9f0d1d27a0f25afeed05a9f9365589060))

## [0.10.0](https://github.com/vanducng/rexboard/compare/v0.9.0...v0.10.0) (2026-10-26)


### Features

* **tui:** a short card bar with a facts block, the key table on the footer, optional Nerd Font icons ([914fa4a](https://github.com/vanducng/rexboard/commit/914fa4a0cfac92d3975b786a024301f7cbc7588d))

## [0.9.0](https://github.com/vanducng/rexboard/compare/v0.8.0...v0.9.0) (2026-10-26)


### Features

* **tui:** back to the last pane, close with confirmation, copy a message or place ([#59](https://github.com/vanducng/rexboard/issues/59)) ([6f03476](https://github.com/vanducng/rexboard/commit/6f03476171aa670d69cb3bfc55ccf6f6e3ca59b1))
* **tui:** vim-friendly keymap from one key table ([05cd17e](https://github.com/vanducng/rexboard/commit/05cd17e960e8cc2264f8f36a415b2e0e99a0c624))

## [0.8.0](https://github.com/vanducng/rexboard/compare/v0.7.0...v0.8.0) (2026-10-26)


### Features

* **tui:** a moving spinner for working and one shape per state ([#58](https://github.com/vanducng/rexboard/issues/58)) ([3b59692](https://github.com/vanducng/rexboard/commit/3b59692018d9b03d7b01428d752cb1837f3a00e3))
* **tui:** extension fields as views, columns and filter keys ([#54](https://github.com/vanducng/rexboard/issues/54)) ([68038c6](https://github.com/vanducng/rexboard/commit/68038c6ca36b070bb80e162573650d5f7a35e9c7))
* **tui:** the full view shows the pane's colours ([#53](https://github.com/vanducng/rexboard/issues/53)) ([301c427](https://github.com/vanducng/rexboard/commit/301c4277723da09a05ead4490bd8a5745fe4b103))


### Bug Fixes

* **tui:** the cut mark keeps a coloured row's band ([#55](https://github.com/vanducng/rexboard/issues/55)) ([59c5f8e](https://github.com/vanducng/rexboard/commit/59c5f8ec1361aa42222183924d30e13a34c07727))

## [0.7.0](https://github.com/vanducng/rexboard/compare/v0.6.2...v0.7.0) (2026-10-26)


### Features

* **cli:** rexboard ext ls, test and new, with an example extension ([#52](https://github.com/vanducng/rexboard/issues/52)) ([4031238](https://github.com/vanducng/rexboard/commit/40312384d74f048fd398895b3aec63dd7164bc54))
* **serve:** run tool extensions over a versioned contract ([#46](https://github.com/vanducng/rexboard/issues/46)) ([f241b20](https://github.com/vanducng/rexboard/commit/f241b2009f6280e4e40a7a119cacca80ddf95c70))
* **tui:** space opens a live full-screen view of the pane ([#50](https://github.com/vanducng/rexboard/issues/50)) ([2956df0](https://github.com/vanducng/rexboard/commit/2956df0a5b0c422846668a5cfda9bcf0243732d7))


### Bug Fixes

* **cli:** rexboard upgrade refreshes the brew tap first ([#51](https://github.com/vanducng/rexboard/issues/51)) ([81f68c5](https://github.com/vanducng/rexboard/commit/81f68c5b74982cb9ac04e476fda4ca567c241b1b))

## [0.6.2](https://github.com/vanducng/rexboard/compare/v0.6.1...v0.6.2) (2026-10-26)


### Bug Fixes

* **tui:** answer a tmux seat whose dialog reports twice, and match the whole dialog ([#47](https://github.com/vanducng/rexboard/issues/47)) ([89589cc](https://github.com/vanducng/rexboard/commit/89589cc12729e8793f1c12cf7600f5564319d1eb))

## [0.6.1](https://github.com/vanducng/rexboard/compare/v0.6.0...v0.6.1) (2026-10-26)


### Bug Fixes

* **tui:** drop the pane tail after an answer ([#44](https://github.com/vanducng/rexboard/issues/44)) ([920a5ef](https://github.com/vanducng/rexboard/commit/920a5ef83934a82997833c05bcfb8826b741e76c))

## [0.6.0](https://github.com/vanducng/rexboard/compare/v0.5.0...v0.6.0) (2026-10-26)


### Features

* **tui:** a turn card at the bottom shows how each agent's turn ended ([#40](https://github.com/vanducng/rexboard/issues/40)) ([7987416](https://github.com/vanducng/rexboard/commit/79874167e0da074914814774b6c3a0e8daa2c42b))
* **tui:** answer an agent from the board with a guarded answer mode ([#42](https://github.com/vanducng/rexboard/issues/42)) ([d9e72ed](https://github.com/vanducng/rexboard/commit/d9e72eda00ce1b1cff28bffce29683bc5d155c1e))

## [0.5.0](https://github.com/vanducng/rexboard/compare/v0.4.0...v0.5.0) (2026-10-26)


### Features

* **tui:** peek reads only the end of a pane, within 3 seconds ([#37](https://github.com/vanducng/rexboard/issues/37)) ([fdd7fc1](https://github.com/vanducng/rexboard/commit/fdd7fc1076ff7f3af629e551cee129b0ab254194))

## [0.4.0](https://github.com/vanducng/rexboard/compare/v0.3.0...v0.4.0) (2026-10-26)


### Features

* **cli:** rexboard upgrade upgrades the way rexboard was installed ([#36](https://github.com/vanducng/rexboard/issues/36)) ([e7bec12](https://github.com/vanducng/rexboard/commit/e7bec1204f7d9ae29ee8c8f4a427ee39dfbc1b62))
* **tui:** c hides and shows the detail pane ([#33](https://github.com/vanducng/rexboard/issues/33)) ([e81e033](https://github.com/vanducng/rexboard/commit/e81e0330fdbdf5763cde5357cb470e95a29da76f))


### Bug Fixes

* **serve:** restart an older or silent serve after an upgrade ([#31](https://github.com/vanducng/rexboard/issues/31)) ([37557f4](https://github.com/vanducng/rexboard/commit/37557f408080503cabcda5452b247511dd419b3b))
* **tui:** show c pane at 80 columns while the pane is hidden ([#35](https://github.com/vanducng/rexboard/issues/35)) ([f6aed9c](https://github.com/vanducng/rexboard/commit/f6aed9cbdcb3bb9604b0db1293cfeef92ba1ef37))

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
