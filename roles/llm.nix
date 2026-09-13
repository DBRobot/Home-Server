{ ... }:
{
  # The language model and its gateway. Wants the ram and the instruction
  # sets; the placement program will read those from the box's facts.
  imports = [
    ../modules/llama-cpp.nix
    ../modules/llm.nix
  ];
}
