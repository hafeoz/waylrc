### Which asset to download?

| If you are ... | ... then download                                                                |
|----------------|----------------------------------------------------------------------------------|
| Linux (64 bit) | `waylrc-x86_64-unknown-linux-musl`                                               |
| Linux (32 bit) | `waylrc-i686-unknown-linux-gnu`                                                  |
| idk            | Run `printf 'waylrc-%s' "$(gcc -dumpmachine)"` and find the asset with such name |

If no asset with your architecture is listed, you can compile yourself as shown in [README](https://github.com/hafeoz/waylrc/blob/master/README.md).

### Supply chain security

Assets attached to this release is compiled by GitHub Action using [`hafeoz/rust-build-release-workflow`](https://github.com/hafeoz/rust-build-release-workflow).
They are cryptographically signed using [GitHub artifact attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds) to establish the build's provenance, including the specific workflow file and workflow run that produced the artifact.

To verify the asset, run:
```shell
gh attestation verify PATH/TO/ARTIFACT -R hafeoz/waylrc
```

Each asset has been built twice via [reprotest](https://salsa.debian.org/reproducible-builds/reprotest) checking [bit-by-bit reproducibility](https://reproducible-builds.org/), although [do note that not all variations are enabled](https://github.com/hafeoz/rust-build-release-workflow?tab=readme-ov-file#caveats-and-security-considerations) and variations are expected to be seen in the wild:
