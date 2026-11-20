# A.8 **sec**

## Fixes

Exec 7; missed 10; A13

## Change

**Atomic `edit` and `write`:** temp file in the same directory (`O_EXCL`, `O_NOFOLLOW` kept), `fsync`, rename, `fsync` the directory, mode kept.
Hard links are split; README says so. `08` §6.3.

## Done when

A write failing halfway (test hook) leaves the original byte-identical and no temp file; mode `0640` kept; a symlink target still refused.
**S**

## Lands on

gate3

