# NixOS VM test for nixosModules.web: web serves its dashboard through the
# firewall, as its own user, configured from the module's options.
{ self, pkgs }:
pkgs.testers.runNixOSTest {
  name = "screentime-web-module";

  nodes = {
    server = {
      imports = [ self.nixosModules.web ];
      services.nats = {
        enable = true;
        jetstream = true;
      };
      services.screentime-web = {
        enable = true;
        natsUrl = "nats://localhost:4222";
        listenAddress = "0.0.0.0";
        openFirewall = true;
        cloudflareAccess = {
          team = "example-team";
          aud = "example-aud";
        };
      };
    };
    client = { };
  };

  testScript =
    { nodes, ... }:
    let
      port = toString nodes.server.services.screentime-web.port;
    in
    ''
      start_all()
      server.wait_for_unit("screentime-web.service")
      server.wait_for_open_port(${port})

      # Runs as its own system user, with CF Access configured from the options.
      server.succeed("test \"$(systemctl show -P User screentime-web)\" = stweb")
      env = server.succeed("systemctl show -P Environment screentime-web")
      assert "CF_ACCESS_TEAM=example-team" in env, env
      assert "CF_ACCESS_AUD=example-aud" in env, env

      # openFirewall lets another machine reach the dashboard.
      client.wait_for_unit("multi-user.target")
      client.succeed("curl -sf http://server:${port}/ | grep -qi '<html'")
    '';
}
