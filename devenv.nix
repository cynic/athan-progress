{
  pkgs,
  lib,
  config,
  ...
}:
{
  languages.rust.enable = true;

  packages = with pkgs; [
    git
    lsof
    ripgrep
    pango
  ]
  ++ lib.optionals stdenv.hostPlatform.isLinux [ pkgs.inotify-tools ];
}
