# Changelog

## [Unrelease]

### Fixed

- Added now required `failure_message` to hook results with `fail`

## [1.0.2] - 2024-04-21

### Fixed

- Race condition when restarting a node while holding alot of invoices. Holdstate of invoices previously in ACCEPTED state can now temporarily be in OPEN state during a node restart. We would previously CANCEL invoices if the amount of sats in ACCEPTED htlcs was dropping below the amount of the invoice.