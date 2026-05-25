# nix/modules/nixos.nix — auto-generated from lava-core.caixa.lisp
# description: "Typed primitive layer for the lava suite. Tatara-lisp + Rust DSL frontend for magma. Brazilian-Portuguese for the substance magma flows as. Sits on pleme-io/magma as the tatara equivalent of pangea-core."
{ config, lib, pkgs, ... }:
let
  cfg = config.services.lava-core;
in {
  options.services.lava-core = {
    enable = lib.mkEnableOption "lava-core";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.lava-core or null;
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
