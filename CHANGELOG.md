# Changelog

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