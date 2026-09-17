{ config, ... }:
{
  # The language model and its gateway. Wants the ram and the instruction
  # sets; the placement program will read those from the box's facts.
  imports = [
    ../modules/llama-cpp.nix
    ../modules/llm.nix
  ];
  dd.home.services = [
    {
      name = "Chat";
      url = "https://llm.${config.dd.domain}/";
      description = "An assistant that runs here, not in someone else's cloud.";
      icon = "chat";
      color = "#2f9e6f";
      rank = 40;
    }
  ];
}
