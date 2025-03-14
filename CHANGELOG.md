# Changelog

## [4.0.0] - 2025-03-11

### Changed

- differentiate between "soft" and "hard" expire. Previously holdinvoice would cancel HTLC's even if it was still possible to settle
- less usage of cln's datastore by not storing expiry value for ACCEPTED state and instead read it from plugin state
- after plugin start wait for 10s before processing rpc commands to give the plugin a chance to process HTLC's during a node restart
- upgraded dependencies

### Fixed

- use a constant amount of rpc connections instead of a proportional amount to the number of HTLC's held, fixes os error 11 crash when holding more HTLC's than the network connections limit


## [3.1.1] - 2024-12-10

### Changed

- upgrade dependencies

## [3.1.0] - 2024-09-23

### Added
- nix flake (thanks to @RCasatta)

### Changed
- updated dependencies

## [3.0.0] - 2024-06-16

### Changed

- Merged `primitives.proto` into `hold.proto`. It was already very small and conflicting with CLN's `primitives.proto`
- Renamed proto package name from `cln` to `hold` so this plugin can stand alone and work together with `cln-grpc`


## [2.0.0] - 2024-06-05

### Added

- Newly supported invoice fields ``exposeprivatechannels`` and ``created_index``

### Changed

- Error on not decodeable ``fallbacks`` in ``HoldInvoiceRequest``'s instead of skipping them
- ``HoldState`` is now a primitive inside primitives.proto
- ``HoldState`` is now consistently UPPERCASE everywhere if represented as a string

### Removed

- ``DecodeBolt11``, it is now supported by ``cln-grpc`` to have routes in the decode/decodepay responses

### Fixed

- Crash in cln v24.05 caused by missing `failure_message` in hook response with `result`:`fail`


## [1.0.2] - 2024-04-21

### Fixed

- Race condition when restarting a node while holding alot of invoices. Holdstate of invoices previously in ACCEPTED state can now temporarily be in OPEN state during a node restart. We would previously CANCEL invoices if the amount of sats in ACCEPTED htlcs was dropping below the amount of the invoice.