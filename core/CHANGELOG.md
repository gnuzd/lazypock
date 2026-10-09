# Changelog

## [0.21.0](https://github.com/gnuzd/lazypock/compare/v0.20.0...v0.21.0) (2026-10-09)


### Features

* **ci:** publish the release as a Docker Hub image ([1bc9f74](https://github.com/gnuzd/lazypock/commit/1bc9f74c343511bf583346024f861e9ebc806181))
* **ci:** publish the release as a Docker Hub image ([08d1019](https://github.com/gnuzd/lazypock/commit/08d1019176cf84df779f06a531f0e393a85ca8ed))

## [0.20.0](https://github.com/gnuzd/lazypock/compare/v0.19.0...v0.20.0) (2026-10-04)


### Features

* **logs:** status filter, retention auto-clean, confirm cleanup ([792c6fd](https://github.com/gnuzd/lazypock/commit/792c6fdd91c48f78d840f6ddde287c73b1b36728))
* **studio:** slide in the media rules available-fields panel ([2a0573b](https://github.com/gnuzd/lazypock/commit/2a0573b617faf5e102e8d580be4b0cd54818db97))


### Bug Fixes

* **files:** add castore so outbound TLS has a trust store ([37abed1](https://github.com/gnuzd/lazypock/commit/37abed1fbdb52890dd25f82cae9bd9427cb76975))
* **files:** send Content-Length on S3/R2 PUTs ([d89e5a0](https://github.com/gnuzd/lazypock/commit/d89e5a0cafd2344089276bb39a5f9731a650ddff))
* **studio:** derive the cron action from the active tab ([2b41715](https://github.com/gnuzd/lazypock/commit/2b41715bef6970f3dd45611b360fe91f383009d1))

## [0.19.0](https://github.com/gnuzd/lazypock/compare/v0.18.0...v0.19.0) (2026-10-04)


### Features

* **studio:** dedicated image picker + keep the insert menu in view ([840ace6](https://github.com/gnuzd/lazypock/commit/840ace6382b44dac5bd2f276c8ba04d44a9b9e91))
* **studio:** image insert dropdown (upload or pick from library) ([e17fe1b](https://github.com/gnuzd/lazypock/commit/e17fe1bab85574e2760d3901251261ea0a99047d))
* **studio:** refresh the richtext editor toolbar ([b2ff107](https://github.com/gnuzd/lazypock/commit/b2ff107eecd10ae3197e3b128aa5827e67fc809b))
* **studio:** richtext toolbar + shared image/file picker ([4cf837a](https://github.com/gnuzd/lazypock/commit/4cf837a157bbc018c4f266d8602659f4c8d30058))
* **studio:** use the image picker for file fields too ([4538b76](https://github.com/gnuzd/lazypock/commit/4538b769229ae05af3471f1258280c96ecc57b52))


### Bug Fixes

* **files:** configurable local storage path + 404 for missing objects ([f36919e](https://github.com/gnuzd/lazypock/commit/f36919e70d6e46ce85a4e23a9b1afc8277fc2f3b))
* **files:** configurable local storage path + 404 for missing objects ([837a512](https://github.com/gnuzd/lazypock/commit/837a512ebfccc84a748c0640729af2da8ffdb690))

## [0.18.0](https://github.com/gnuzd/lazypock/compare/v0.17.1...v0.18.0) (2026-10-04)


### Features

* **files:** list/delete access rules with uploader ownership ([e219b52](https://github.com/gnuzd/lazypock/commit/e219b528a030c77004fa502baed794cccb0d2b6f))

## [0.17.1](https://github.com/gnuzd/lazypock/compare/v0.17.0...v0.17.1) (2026-10-04)


### Bug Fixes

* **images:** read image dimensions on ImageMagick 6; make CI fail loudly ([93cc801](https://github.com/gnuzd/lazypock/commit/93cc8012ff48578e64acc0c74ee101ff81dd8a31))

## [0.17.0](https://github.com/gnuzd/lazypock/compare/v0.16.0...v0.17.0) (2026-10-04)


### Features

* **files:** sharper grid thumbnails (320px `small` preset) ([f949aeb](https://github.com/gnuzd/lazypock/commit/f949aeb42ba7063d0f50fad6f58d4fb160f41210))
* **studio:** grid/list views + in-app delete confirmation ([561a992](https://github.com/gnuzd/lazypock/commit/561a992cce038d067fc1057403e35d331621b846))
* **studio:** media library pagination + image skeletons ([32c5cf0](https://github.com/gnuzd/lazypock/commit/32c5cf012f2ea925420da4eb10b8c1192ff17214))
* **studio:** media library under Settings, bigger modals, detail view ([df538f1](https://github.com/gnuzd/lazypock/commit/df538f154560ea77d67a10e8b470c248d69205e3))


### Bug Fixes

* **auth:** give auth collections their system fields ([dfcecd7](https://github.com/gnuzd/lazypock/commit/dfcecd74bb146a16be123a570e4eb96cf5d14222))
* **auth:** include collectionName/collectionId in auth responses ([b7b228d](https://github.com/gnuzd/lazypock/commit/b7b228d0d4f62f6549cb6345fb95dcce0612e970))
* **schema:** system fields cannot be dropped ([f109490](https://github.com/gnuzd/lazypock/commit/f109490309df4f0720028d5ff70b8640ad8b9c62))

## [0.16.0](https://github.com/gnuzd/lazypock/compare/v0.15.2...v0.16.0) (2026-10-03)


### Features

* **files:** deletion outbox + reaper (G7) ([d66c00a](https://github.com/gnuzd/lazypock/commit/d66c00a4c463e5eaeedf50f1286dcee74390025e))
* **files:** direct-to-R2 uploads (presign + complete) ([1c0e2a7](https://github.com/gnuzd/lazypock/commit/1c0e2a7035434d7c575d19e2461032c9e1910b51))
* **files:** image engine abstraction, presets and variants ([f348aa9](https://github.com/gnuzd/lazypock/commit/f348aa9ffbf5c56f1f54e4ce6e20d7aa29af2fb5))
* **files:** image upload & scaling plan (P0-P5) ([ef17412](https://github.com/gnuzd/lazypock/commit/ef1741284f6aad8144cd7a263920de98b541f041))
* **files:** operations CLI, reference tracking, delete guard, health ([ca714dc](https://github.com/gnuzd/lazypock/commit/ca714dcb934d0c6317a48ffddbda983fe1f37c39))
* **files:** real S3/R2 storage adapter + storage settings ([bbc5666](https://github.com/gnuzd/lazypock/commit/bbc5666204f77e8527c9873309fe96728fb9614d))
* **files:** richtext image picker (TipTap) + variant/CDN URLs ([1fdc4d7](https://github.com/gnuzd/lazypock/commit/1fdc4d77e1d70a9ad1d7d09f3bb6e9202165a347))
* **files:** settings-backed upload policy, scoped body limit, AVIF ([38380f1](https://github.com/gnuzd/lazypock/commit/38380f17f29d9a9ca734fa231fb8253407223a9c))
* **studio:** shared media library (pick or upload) + media page + storage page fix ([d0da341](https://github.com/gnuzd/lazypock/commit/d0da341dcb872bf07d6af6ddf3c4a9dc57a76f37))


### Bug Fixes

* **files:** bound image work, stream uploads, fix variant format ([7439164](https://github.com/gnuzd/lazypock/commit/74391641c03e6d0e2553c781e533c0b3c108b34f))

## [0.15.2](https://github.com/gnuzd/lazypock/compare/v0.15.1...v0.15.2) (2026-10-02)


### Bug Fixes

* **auth:** harden the OAuth2 redirect flow (XSS, token leak, session store) ([74901e7](https://github.com/gnuzd/lazypock/commit/74901e7ad9595182eda102bd6aa84ebaf2df761b))
* **auth:** harden the OAuth2 redirect flow (XSS, token leak, session store) ([17583cf](https://github.com/gnuzd/lazypock/commit/17583cfcfa57d091223af1c5a7b2d85020c50b6c))
* **db:** stop the connect CaseClauseError from a raw ?ssl= parameter ([6083333](https://github.com/gnuzd/lazypock/commit/6083333cd162d141e3e947938d5fcc48143abd94))
* **db:** stop the connect CaseClauseError from a raw ?ssl= parameter ([a13681f](https://github.com/gnuzd/lazypock/commit/a13681f9098bcc71b5459a1a6a0e69f6d876c8c5))

## [0.15.1](https://github.com/gnuzd/lazypock/compare/v0.15.0...v0.15.1) (2026-10-02)


### Bug Fixes

* **db:** honor sslmode in DATABASE_URL for Postgres TLS ([3849a8a](https://github.com/gnuzd/lazypock/commit/3849a8adbf563c647838b0a9e464c278d2361462))
* **db:** honor sslmode in DATABASE_URL for Postgres TLS ([a7ccba7](https://github.com/gnuzd/lazypock/commit/a7ccba743576b1da7b742690ec120a7520c9948e))

## [0.15.0](https://github.com/gnuzd/lazypock/compare/v0.14.1...v0.15.0) (2026-09-29)


### Features

* **auth:** configurable token TTL and optional dedicated signing secret ([ad230ba](https://github.com/gnuzd/lazypock/commit/ad230baf0ca9f43dbbcca8672f099ad3e6a2c5b1))
* **auth:** implement email-change flow (request/confirm endpoints) ([11435f1](https://github.com/gnuzd/lazypock/commit/11435f1dd3f97f0094f48f98218ffa02f0bfe11d))
* **auth:** implement email-change flow (request/confirm endpoints) ([57ffb97](https://github.com/gnuzd/lazypock/commit/57ffb9756f1d054ce92cf61bc6cc486a75a8a4fd))


### Bug Fixes

* **security:** constant-time API key verification ([b0e33f2](https://github.com/gnuzd/lazypock/commit/b0e33f20faf199e3f539e606a5aa9beddddddfcf))
* **security:** validate uploads by extension allowlist + magic bytes ([82fc8ea](https://github.com/gnuzd/lazypock/commit/82fc8ea4bc273a91c658ab37f91cc855e5c4cb37))


### Performance Improvements

* **filters:** memoize parsed filter ASTs in ETS ([b8942e9](https://github.com/gnuzd/lazypock/commit/b8942e99f076c5660aef38d45038b3ff360b975e))

## [0.14.1](https://github.com/gnuzd/lazypock/compare/v0.14.0...v0.14.1) (2026-09-27)


### Bug Fixes

* **filter:** relation dot-paths + IS [NOT] NULL comparisons ([f448e42](https://github.com/gnuzd/lazypock/commit/f448e42e02e7e2ea9f41eab59cda3e5c007bcbfb))
* **filter:** relation dot-paths + IS [NOT] NULL comparisons; fix(studio): camelCase field names ([4b7ee30](https://github.com/gnuzd/lazypock/commit/4b7ee30d3ee5f3499458c5b462445644baeb4ea0))
* **studio:** allow camelCase field names ([ebb072c](https://github.com/gnuzd/lazypock/commit/ebb072c4c54ec47dbb4ac0a6a33955f292511432))

## [0.14.0](https://github.com/gnuzd/lazypock/compare/v0.13.0...v0.14.0) (2026-09-26)


### Features

* **api:** accept backup archives over HTTP with a scoped body limit ([b4f5475](https://github.com/gnuzd/lazypock/commit/b4f5475560ccbfe08ef386577c8dec084f4b760c))
* **backup:** include uploaded files in the archive, and preview archives ([78d34f7](https://github.com/gnuzd/lazypock/commit/78d34f7a419a0a6bee45ff95da19bea1aa3b47ea))
* **backup:** stream large databases as an NDJSON archive ([b048682](https://github.com/gnuzd/lazypock/commit/b048682403ef814ec0aeda5c351243c9c15a2417))
* **backup:** stream large databases as an NDJSON archive ([1b3dc52](https://github.com/gnuzd/lazypock/commit/1b3dc528d71d1c6da7c2587780fd4c7863757244))
* **studio:** archive backup/restore with progress and a large-import gate ([8da4b71](https://github.com/gnuzd/lazypock/commit/8da4b715dfe9c183cb4687d764a550522da3c2ad))

## [0.13.0](https://github.com/gnuzd/lazypock/compare/v0.12.1...v0.13.0) (2026-09-24)


### Features

* **import:** atomic restore + one-click rollback ([cbd18d7](https://github.com/gnuzd/lazypock/commit/cbd18d776fbaf09979e216d0a23bebbad29e4d18))
* **import:** atomic restore + one-click rollback ([3a1d3b6](https://github.com/gnuzd/lazypock/commit/3a1d3b6fe356f48769ded6081f12557351c9cd80))
* **security:** restrict + audit + re-auth import/export/rollback ([82068e9](https://github.com/gnuzd/lazypock/commit/82068e97d3fcae1d8e10d87c4a7d4c19a7e9893d))
* **security:** superuser-only import/export — guard tests, audit, re-auth ([1a76b92](https://github.com/gnuzd/lazypock/commit/1a76b923aebb6918b191737d6d6d2db11572a1c4))
* **studio:** add a "Copy AI prompt" button to Import and Backups ([e54a22a](https://github.com/gnuzd/lazypock/commit/e54a22a5aaeefb07cc45e69fe4124813b9bf7057))
* **studio:** confirm password for import / restore / rollback ([eb2b997](https://github.com/gnuzd/lazypock/commit/eb2b997337094d83bc141b20b969d42ff2d73af8))
* **studio:** Copy AI prompt for import/restore ([e4bfa8e](https://github.com/gnuzd/lazypock/commit/e4bfa8e50308bd864a5c4c839d6280896935f08c))


### Bug Fixes

* collection deletion (unmanaged 400 + SDK fire-and-forget race) ([79dd06c](https://github.com/gnuzd/lazypock/commit/79dd06ce550c58888d491caba8ab27733cf6e47c))
* **core:** allow deleting unmanaged collections (they were undeletable) ([9ef031f](https://github.com/gnuzd/lazypock/commit/9ef031f23a3c4abc3c0d1d06fccd1c4eefc7e5ef))
* **core:** make the auth-collection email field unique by default ([9af5c76](https://github.com/gnuzd/lazypock/commit/9af5c76a877fd741b28f08d0cd4fa5fa358e5d59))
* **core:** make the auth-collection email field unique by default ([6137640](https://github.com/gnuzd/lazypock/commit/61376408105b4f9112b1e3fc826cde8f4d4eb125))
* **import:** report the real error instead of a 25P02 cascade ([b3a70b1](https://github.com/gnuzd/lazypock/commit/b3a70b1088857186bcc04c199c628e6d24703fdd))
* **studio:** animate only .loading-spinner, not the .loading state hook ([5148846](https://github.com/gnuzd/lazypock/commit/5148846e55501ece01889d8bc89016cd59f7cb7c))
* **studio:** await collection delete instead of racing the refresh ([b33ddbd](https://github.com/gnuzd/lazypock/commit/b33ddbd28e850d60ec4d447a0bb29f1741c51263))
* **studio:** clear deleted collection's table + modal for rollback ([4eb7097](https://github.com/gnuzd/lazypock/commit/4eb70974c95be436981251870a7e33d944e9cc50))
* **studio:** clear the deleted collection's table + modal for rollback ([1280008](https://github.com/gnuzd/lazypock/commit/1280008ea773da2a4bba9dfdf6d2867864293787))
* **studio:** give icon buttons a real extra-small size ([766cf5e](https://github.com/gnuzd/lazypock/commit/766cf5e4f5c87084e2def20a8000a1cec21fc434))
* **studio:** keep the button label from spinning with the loader ([8179036](https://github.com/gnuzd/lazypock/commit/817903650c1259658bb60033ef69e3ae8b1f9f1e))
* **studio:** unblock index creation and stabilize the record table ([7afad21](https://github.com/gnuzd/lazypock/commit/7afad217903cd89ea0a337cc60a9927b1797d100))
* **studio:** unblock index creation and stabilize the record table ([9862aa3](https://github.com/gnuzd/lazypock/commit/9862aa3a087cf9ae6220590bc3eaf3753a3e87ff))

## [0.12.1](https://github.com/gnuzd/lazypock/compare/v0.12.0...v0.12.1) (2026-09-24)


### Bug Fixes

* **ci:** parse the coverage table in both Elixir 1.17 and 1.18+ formats ([2952119](https://github.com/gnuzd/lazypock/commit/2952119ba2a179cde69e1eaa5898a677bc8cf2ff))
* **rules:** fail closed on rules the enforcer cannot evaluate ([5edd370](https://github.com/gnuzd/lazypock/commit/5edd370b375b1678ab7a6685c2221adbc568650c))
* **rules:** fail closed on rules the enforcer cannot evaluate ([87e1ef2](https://github.com/gnuzd/lazypock/commit/87e1ef2e7f57bf58d572fe670fb4cb09724fb148))

## [0.12.0](https://github.com/gnuzd/lazypock/compare/v0.11.0...v0.12.0) (2026-09-24)


### Features

* **collections:** preview endpoint for the view builder ([15e60c8](https://github.com/gnuzd/lazypock/commit/15e60c8a4661dd8a38f9af3a58c026b0579a345a))
* **schema:** no-code view builder query generator ([93e7f66](https://github.com/gnuzd/lazypock/commit/93e7f66f13d7f02fdf1ad27a6c26dd5ede5dbbac))
* **schema:** no-code view builder query generator ([7753252](https://github.com/gnuzd/lazypock/commit/7753252e874519fc914ac22693176a265b5f6f3d))
* **schema:** wire the view builder into DDL and the collections API ([bbfa76c](https://github.com/gnuzd/lazypock/commit/bbfa76c5da4f7a8bca49191d37073eae44e925ba))
* **studio:** no-code view builder UI ([251dbaa](https://github.com/gnuzd/lazypock/commit/251dbaae56ceca4702251ab869f77298280e8fed))


### Bug Fixes

* **collections:** allow updating a view collection from the Studio ([a0029d4](https://github.com/gnuzd/lazypock/commit/a0029d4fb3d8ee247ac67bd2748b185f9e5c69fc))
* **collections:** allow updating a view collection from the Studio ([e6f931b](https://github.com/gnuzd/lazypock/commit/e6f931b4b4020d0a07b0972151aefdef573f013c))
* **studio:** show table column names verbatim ([d818cb7](https://github.com/gnuzd/lazypock/commit/d818cb793fc0e49388b0db51520c964a6a34c8f7))
* **studio:** show table column names verbatim ([9c95db0](https://github.com/gnuzd/lazypock/commit/9c95db0df1994ce099552a383ea0fbbf607e5e61))
* **studio:** view builder feedback + view realtime test coverage ([125a3f5](https://github.com/gnuzd/lazypock/commit/125a3f57e7e584cad7ecd08b3c63327f10d02e33))
* **studio:** view builder feedback + view realtime test coverage ([4b31e31](https://github.com/gnuzd/lazypock/commit/4b31e315cdd52f9e6e6471561dcc0fb6426ad30e))

## [0.11.0](https://github.com/gnuzd/lazypock/compare/v0.10.2...v0.11.0) (2026-09-22)


### Features

* **filters:** PocketBase ? operators for array fields ([3770177](https://github.com/gnuzd/lazypock/commit/37701779810dc16e491f466f591043b9404b8c91))
* **filters:** PocketBase ? operators for array fields ([0117316](https://github.com/gnuzd/lazypock/commit/0117316df4c5d81a21ad409e083ea7d1e6f33cc3))
* **studio:** add filter, sort and always-on record count to record tables ([0b6eb16](https://github.com/gnuzd/lazypock/commit/0b6eb16c4272c3360ee6b4cbf58efd8e6c77b535))


### Bug Fixes

* data import/export support camelCase and pb latest version ([8e570f5](https://github.com/gnuzd/lazypock/commit/8e570f5d4dbaf3e70437bc919e555b15192f05c5))
* **schema:** keep field names verbatim as DB columns + system timestamps ([5f3eac2](https://github.com/gnuzd/lazypock/commit/5f3eac2b39c337f2c64304e3dd7b64117ebef2a9))
* **schema:** keep field names verbatim as DB columns + system timestamps ([13c8d6d](https://github.com/gnuzd/lazypock/commit/13c8d6d472fe121ebd7e9736d09c407bf8278715))

## [0.10.2](https://github.com/gnuzd/lazypock/compare/v0.10.1...v0.10.2) (2026-09-20)


### Bug Fixes

* reject legacy options ([7152abd](https://github.com/gnuzd/lazypock/commit/7152abda40754c11f918cbe6d904f95a99318d47))
* test validate_field ([0333017](https://github.com/gnuzd/lazypock/commit/03330177b28757dc12aeb1bceec688d78d1c6fce))
* update backup/restore pb type ([04c1c5d](https://github.com/gnuzd/lazypock/commit/04c1c5dfc8c8a5a30d60b08f9b401386e13836e1))

## [0.10.1](https://github.com/gnuzd/lazypock/compare/v0.10.0...v0.10.1) (2026-09-19)


### Bug Fixes

* missing form fields settings import ([b622d73](https://github.com/gnuzd/lazypock/commit/b622d737a4d5eac42eafe6ffe1901f3a2752b9bc))
* missing form fields settings import ([d059af5](https://github.com/gnuzd/lazypock/commit/d059af5721c97c7f8420eb6e73d938a99d16531b))

## [0.10.0](https://github.com/gnuzd/lazypock/compare/v0.9.0...v0.10.0) (2026-09-19)


### Features

* update super form ([63d8611](https://github.com/gnuzd/lazypock/commit/63d86116777e0a47e74f25e8b20c13187159116e))


### Bug Fixes

* polish ui ([0656bc5](https://github.com/gnuzd/lazypock/commit/0656bc58f19167e293d49baeea0b3987683debd2))

## [0.9.0](https://github.com/gnuzd/lazypock/compare/v0.8.1...v0.9.0) (2026-09-01)


### Features

* **schema:** support autodate field type with write-path maintenance ([28cbb17](https://github.com/gnuzd/lazypock/commit/28cbb17e0a1c5486714748c2c9d0fcbf81f3e021))
* **schema:** support autodate field type with write-path maintenance ([7b3e872](https://github.com/gnuzd/lazypock/commit/7b3e8720c9563521451368660e2d95be19fb38d4))


### Bug Fixes

* **migrations:** fail fast on unresolvable relation targets ([0fb9a85](https://github.com/gnuzd/lazypock/commit/0fb9a8571c2e50d9c72c7acc6f83188b06feefd0))
* **migrations:** fail fast on unresolvable relation targets ([9bd81d4](https://github.com/gnuzd/lazypock/commit/9bd81d43d4fe24dbfb0ddafa6cedca34d91d91bf))

## [0.8.1](https://github.com/gnuzd/lazypock/compare/v0.8.0...v0.8.1) (2026-08-31)


### Bug Fixes

* **core,studio:** relation dropdowns for migrated tables, clean test DB logs, docs ([c80fd11](https://github.com/gnuzd/lazypock/commit/c80fd11bc41589d67d4344c5a2a184ef70a59983))

## [0.8.0](https://github.com/gnuzd/lazypock/compare/v0.7.1...v0.8.0) (2026-08-30)


### Features

* **core:** view collections with realtime (PocketBase parity) ([c5f2e92](https://github.com/gnuzd/lazypock/commit/c5f2e9273fdb414a512497018514f306c2a1f832))
* **studio:** collections overview modal (PocketBase parity) ([7d8570c](https://github.com/gnuzd/lazypock/commit/7d8570ccfff95658805b798b17a44b8cfdc724e0))
* **studio:** rebuild ERD on Svelte Flow (zoom/pan, relation edges) ([ac09a0d](https://github.com/gnuzd/lazypock/commit/ac09a0d832d992a2a4afbd6c0b2ca97e145122c9))
* **studio:** view collection UX ([fed70da](https://github.com/gnuzd/lazypock/commit/fed70da60c20dcc1b7ca9ec84bb8da4e5593ad0c))


### Bug Fixes

* **realtime:** own the view-diff ETS snapshot in the Registry ([927cff5](https://github.com/gnuzd/lazypock/commit/927cff51c1cf663f615d4a2ba792035b219353a0))

## [0.7.1](https://github.com/gnuzd/lazypock/compare/v0.7.0...v0.7.1) (2026-08-30)


### Bug Fixes

* **studio:** always request all fields for record reads ([607ad61](https://github.com/gnuzd/lazypock/commit/607ad61569ad0f69ea0751d29bebc58dc83d3f7b))
* **studio:** always request all fields for record reads (stale schema projection drops new fields) ([530bdab](https://github.com/gnuzd/lazypock/commit/530bdaba5d6cee159671703779e48aeb6e4555a0))

## [0.7.0](https://github.com/gnuzd/lazypock/compare/v0.6.2...v0.7.0) (2026-08-29)


### Features

* **docs:** add SvelteKit documentation site for lazypock-ts ([151b12a](https://github.com/gnuzd/lazypock/commit/151b12a6ddfd0ff8b9daa68c0d9b823d19e38321))

## [0.6.2](https://github.com/gnuzd/lazypock/compare/v0.6.1...v0.6.2) (2026-08-28)


### Bug Fixes

* **enforcer:** deny instead of crashing on malformed record id in rule eval ([c1f4ed0](https://github.com/gnuzd/lazypock/commit/c1f4ed023215f5fecfcaf878bb8f04e6595114f5))
* **enforcer:** deny instead of crashing on malformed record id in rule eval ([25581d1](https://github.com/gnuzd/lazypock/commit/25581d1a27c169c2b92715215446558f352ad4ae))

## [0.6.1](https://github.com/gnuzd/lazypock/compare/v0.6.0...v0.6.1) (2026-08-25)


### Bug Fixes

* **cors:** echo requested headers in preflight so X-Connection-Id is allowed ([4bbe37f](https://github.com/gnuzd/lazypock/commit/4bbe37fa5b83ec44751983cca0ac89680db1e179))

## [0.6.0](https://github.com/gnuzd/lazypock/compare/v0.5.1...v0.6.0) (2026-08-25)


### Features

* **migrations:** auto-register raw tables as collections + realtime id support ([63defa8](https://github.com/gnuzd/lazypock/commit/63defa80a64b8a31a2b934d9d9d29ee511e29f0f))


### Bug Fixes

* **migrations:** auto-register raw tables as collections; realtime origin-exclusion for SDK clients ([d80d134](https://github.com/gnuzd/lazypock/commit/d80d1340df9a46930362f7fe21992c3dbbb6d542))

## [0.5.1](https://github.com/gnuzd/lazypock/compare/v0.5.0...v0.5.1) (2026-08-24)


### Bug Fixes

* register collections from user migrations + add backup restore ([fdeae88](https://github.com/gnuzd/lazypock/commit/fdeae88de10e416731d7b141ee7efd924bd9fea1))

## [0.5.0](https://github.com/gnuzd/lazypock/compare/v0.4.5...v0.5.0) (2026-08-23)


### Features

* **realtime:** exclude origin connection, custom channels, admin channel fix ([a6bf6e4](https://github.com/gnuzd/lazypock/commit/a6bf6e41149372d0d55534a30af2978a5801f5e8))
* **realtime:** exclude origin connection, custom channels, admin channel fix ([68b0f30](https://github.com/gnuzd/lazypock/commit/68b0f30a6b5f9d8bc070614437caa075317985be))

## [0.4.5](https://github.com/gnuzd/lazypock/compare/v0.4.4...v0.4.5) (2026-08-22)


### Bug Fixes

* **core:** guard hide-password migration against missing boot-time tables ([e8ce84d](https://github.com/gnuzd/lazypock/commit/e8ce84dbbf45ea9087e21646d726825cbb8ead12))
* **core:** guard hide-password migration against missing boot-time tables ([39206cc](https://github.com/gnuzd/lazypock/commit/39206cc344a9248ff2f6f29a34219e3528b432eb))

## [0.4.4](https://github.com/gnuzd/lazypock/compare/v0.4.3...v0.4.4) (2026-08-22)


### Bug Fixes

* **collections:** users is a normal auth collection, not system ([094866c](https://github.com/gnuzd/lazypock/commit/094866cc0235b41a10634e4c0b18800d696667f8))
* **collections:** users is a normal auth collection, not system ([fca447f](https://github.com/gnuzd/lazypock/commit/fca447fddb46dc391646665b970ba70e9ac08db2))
* **users:** accept `password` alias, hide password fields ([5eb30f6](https://github.com/gnuzd/lazypock/commit/5eb30f681b44579518784e162e26ca62d5f7a3c2))
* **users:** accept password alias, hide password fields ([2392c11](https://github.com/gnuzd/lazypock/commit/2392c1127943f5f4aa6961b066cae0f39df6306d))

## [0.4.3](https://github.com/gnuzd/lazypock/compare/v0.4.2...v0.4.3) (2026-08-22)


### Bug Fixes

* **import:** toast results, drop posts seed, self-heal missing users collection ([fc10943](https://github.com/gnuzd/lazypock/commit/fc109431b72aeb7f4176866296075264ad1ba995))
* **import:** toast results, drop posts seed, self-heal missing users collection ([d805cb7](https://github.com/gnuzd/lazypock/commit/d805cb7b71b9dbebe92d62fea5ee4cd06c727307))

## [0.4.2](https://github.com/gnuzd/lazypock/compare/v0.4.1...v0.4.2) (2026-08-22)


### Bug Fixes

* **import:** keep field names verbatim — no snake/camel conversion ([d195ed5](https://github.com/gnuzd/lazypock/commit/d195ed5dbc455c1f17adf6b6eb10c3eff7073fc5))
* **import:** make PocketBase JSON exports import cleanly + honor deleteMissing for fields ([b0b5429](https://github.com/gnuzd/lazypock/commit/b0b5429d9b4aa3cec63aab1ed908f1927cdda104))
* **import:** PocketBase JSON exports import cleanly + deleteMissing honors fields ([6830c4b](https://github.com/gnuzd/lazypock/commit/6830c4ba243007ec57312c82836f77d036b80450))

## [0.4.1](https://github.com/gnuzd/lazypock/compare/v0.4.0...v0.4.1) (2026-08-22)


### Bug Fixes

* **migrations:** only treat numeric-prefixed files as migration versions ([83bff32](https://github.com/gnuzd/lazypock/commit/83bff3267c90c2abe175db1359162e5293caf181))
* **migrations:** only treat numeric-prefixed files as migration versions ([1ee7beb](https://github.com/gnuzd/lazypock/commit/1ee7beb0ed7dcae221b0ceda627dc528c4cc20c8))

## [0.4.0](https://github.com/gnuzd/lazypock/compare/v0.3.3...v0.4.0) (2026-08-22)


### Features

* **migrations:** run bundled system migrations from inside the binary ([7cbfcca](https://github.com/gnuzd/lazypock/commit/7cbfcca46ecaaeae91190e335d3128058099748d))
* **migrations:** run bundled system migrations from inside the binary ([fcb4e5e](https://github.com/gnuzd/lazypock/commit/fcb4e5ecc4e92a2211a5e869035fdb08272ec137))

## [0.3.3](https://github.com/gnuzd/lazypock/compare/v0.3.2...v0.3.3) (2026-08-22)


### Bug Fixes

* **studio:** document /api proxy in shared client ([755dfea](https://github.com/gnuzd/lazypock/commit/755dfea79cd4e56d1b25b6fe47fef15589ee33b2))

## [0.3.2](https://github.com/gnuzd/lazypock/compare/v0.3.1...v0.3.2) (2026-08-22)


### Bug Fixes

* **core:** reference app-context module in moduledoc ([5925059](https://github.com/gnuzd/lazypock/commit/5925059da15b531294d978ea6cfe1477ad9d9e87))
* **core:** reference app-context module in moduledoc ([725fcb1](https://github.com/gnuzd/lazypock/commit/725fcb13ddfb3c6e52c2df0367277daebf75df4e))

## [0.3.1](https://github.com/gnuzd/lazypock/compare/v0.3.0...v0.3.1) (2026-08-22)


### Bug Fixes

* **core:** document canonical app-context accessor for hook handlers ([38a5a8d](https://github.com/gnuzd/lazypock/commit/38a5a8de711cac86a2b7018f038460a1fabf7e50))
* **core:** document canonical app-context accessor for hook handlers ([c7ceaeb](https://github.com/gnuzd/lazypock/commit/c7ceaeb5ed7900072d819508cdab6fad20bb7708))
