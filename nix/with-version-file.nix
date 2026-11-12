# Wrap a binary that uses bpaf-kit so `--version` reports a git rev, dirty
# flag and date supplied at wrap time instead of compile time. The binary
# itself stays cacheable across commits; only this cheap wrapper changes.
#
#   withVersionFile = pkgs.callPackage ./nix/with-version-file.nix { };
#   withVersionFile {
#     package = my-tool;            # the unwrapped package
#     rev = self.rev or self.dirtyRev or null;
#     dirty = self ? dirtyRev;
#     date = "2026-11-12";          # optional, YYYY-MM-DD
#   }
#
# Contract (see bpaf_kit::version): the wrapper sets <NAME>_VERSION_FILE,
# NAME being the binary name upper-cased with non-alphanumerics as `_`, to a
# file of `key=value` lines (version, rev, dirty, date, target).
{
  lib,
  runCommand,
  makeWrapper,
  writeText,
}:
{
  package,
  name ? package.meta.mainProgram or package.pname,
  rev ? null,
  dirty ? null,
  date ? null,
  version ? null,
}:
let
  envVar =
    lib.toUpper (lib.stringAsChars (c: if builtins.match "[A-Za-z0-9]" c != null then c else "_") name)
    + "_VERSION_FILE";
  lines =
    lib.optional (version != null) "version=${version}"
    ++ lib.optional (rev != null) "rev=${rev}"
    ++ lib.optional (dirty != null) "dirty=${lib.boolToString dirty}"
    ++ lib.optional (date != null) "date=${date}";
  versionFile = writeText "${name}-version.env" (lib.concatLines lines);
in
runCommand "${name}-${package.version or "wrapped"}"
  {
    nativeBuildInputs = [ makeWrapper ];
    meta = (package.meta or { }) // {
      mainProgram = name;
    };
  }
  ''
    install -Dm644 ${versionFile} $out/share/${name}/version.env
    makeWrapper ${lib.getExe' package name} $out/bin/${name} \
      --set ${envVar} $out/share/${name}/version.env
    if [ -d ${package}/share/man ]; then
      mkdir -p $out/share && ln -s ${package}/share/man $out/share/man
    fi
  ''
