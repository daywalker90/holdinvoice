# Changelog

## [5.0.0] - Unreleased

### Changed
- :warning: Upgrade notice: When upgrading from holdinvoice < 5 all previously created unpaid invoices will be just normal CLN invoices and no longer be held. You must either wait for them to be expired/paid, delete them or be fine with them immediately settling.
- :warning: There have been big, breaking, internal changes to handle the creation of holdinvoices where you provide the ``payment_hash`` yourself and reveal the ``preimage`` to the plugin later.
- When close to expiry the plugin might be forced to cancel HTLC's instead of settling them when only a `payment_hash` was provided during invoice creation.
- :warning: The plugin no longer uses CLN's own ``invoice`` command and as such you won't find them in ``listinvoices``.
- Invoices are still stored in CLN's database but in the plugin's own datastore. Holdinvoice states from < v5 were stored under the `holdinvoice` database key. In v5 the state, the invoice and additional metadata will be stored under the `holdinvoice_v2` key.
- Autoclean will be performed on this data with the settings from CLN's autoclean.
- The plugin can now hold HTLC's beyond the invoices expiry and then settle/cancel. It is now only bound by the blockheight expiry of the HTLC's
- `holdinvoice`: `cltv` is no longer mandatory and defaults to ``144``
- The plugin sometimes didn't return a real error, but an ok response with a json rpc error object. It now always returns a real error response but they will always have the `code` as `-32700`
- Give HTLC's 20s (was 10s) for replaying during node restarts before answering to rpc calls
- Instead of ``holdinvoicelookup``, ``holdinvoicesettle`` and ``holdinvoicecancel`` will wait for HTLC' completion with a timeout of now 30s
- ``holdinvoicelookup`` can now be called without a ``payment_hash`` and will return an object called ``holdinvoices`` with an array of objects containing all holdinvoices or the one specified by ``payment_hash`` with the same fields as the ``holdinvoice`` response

### Added
- `holdinvoice-version`: new command that returns the version of holdinvoice used
- `holdinvoice`: `payment_hash` argument. You must provide the ``preimage`` later for settling.
- `holdinvoice`: `exposeprivatechannels` argument aswell as it's default functionality like in CLN's `invoice` command.
- `holdinvoice`: Some optional return fields have been added: ``description``, ``description_hash``, ``preimage``
- `holdinvoicesettle`: new `preimage` argument. If you have only provided a `payment_hash` during invoice creation you must now provide the ``preimage`` here.


### Removed
- `holdinvoice`: `label` argument.
- `holdinvoice`: `fallbacks` argument.
- `holdinvoice`: several return fields are no longer available: ``warning_capacity``, ``warning_offline``, ``warning_deadends``, ``warning_private_unused``, ``warning_mpp``, ``created_index``
- :warning: `holdinvoice-cancel-before-invoice-expiry`: option removed, we can now settle/cancel invoices after expiry. Make sure to remove this option from your config before starting your node or it will not start.


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
