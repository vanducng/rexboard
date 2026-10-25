# Changelog

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
