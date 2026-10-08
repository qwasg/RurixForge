# AI Asset Rights Status

Last updated: 2026-08-25

This file records the current provenance and release status of non-code assets. It is an audit record, not a grant of rights and not legal advice.

## Active character asset

| Field | Current status |
|---|---|
| File | `Resources/Mascot/dragon-chibi-neutral-v4.png` |
| SHA-256 | `7531a1a0a965e605c1c44ac27d07ebb824dab4072f6b522c658ce0a3ea7d0827` |
| Description | White-and-purple chibi dragon character on a transparent background |
| Source statement | Supplied by the project owner, who states that it was created with OpenAI GPT image generation |
| Generator/service | OpenAI GPT image generation; the exact ChatGPT/API product and model have not yet been recorded |
| Prompt and reference inputs | The owner gave character and editing directions and supplied a reference image; the exact prompts and the origin/rights status of every reference input have not yet been recorded |
| Human creative contribution | The owner selected the direction and requested iterative chibi-character changes, including removal of the earlier visible mark; the copyright significance of those choices has not been determined |
| Visible OpenAI Blossom-like mark | Removed in this v4 asset |
| File metadata check | 1536×1024 RGBA PNG, alpha present; no xattr and no visible OpenAI/GPT/C2PA/author/GPS strings found during the 2026-08-24 audit |
| Similarity review | No specific source work has been identified, but no formal similarity clearance has been completed |
| Redistribution/open-source license | Excluded from the MIT License; no separate right to reuse, modify, sublicense, or redistribute the artwork is granted. See `ASSET_LICENSE.md` |

Metadata cleanliness does not prove copyright ownership, non-infringement, or compliance with the generator's terms. Before public or open-source release, the publisher should preserve the generator name, generation date, applicable terms, prompts, reference inputs, and the basis for using every reference input.

The [OpenAI Terms of Use](https://openai.com/policies/terms-of-use/) reviewed on 2026-08-25 state that, as between the user and OpenAI and to the extent permitted by applicable law, the user owns Output and OpenAI assigns any rights it has in that Output. The same terms make the user responsible for having the necessary rights in Input and note that outputs may not be unique. This project has not yet confirmed which OpenAI product/account terms governed the actual generation event, so that general terms statement is not recorded as final clearance for this asset.

Japanese Agency for Cultural Affairs guidance explains that a purely autonomous AI output may not qualify as a copyrighted work, while an output can qualify when a human uses AI as a tool with creative intent and creative contribution; the conclusion depends on the specific generation process. This record therefore does not assume that copyright automatically exists merely because the owner selected and iteratively revised an AI output.

## AppIcon

`Resources/AppIcon.png` and `Resources/AppIcon.icns` are generated derivatives of the active character asset. Their release status follows the character asset and they are expressly excluded from the repository's MIT License.

## Rubber-duck sound

The rubber-duck sound is synthesized locally by project source code at runtime and is not a separately bundled third-party audio recording. Its implementation follows the software-code license selected by the publisher.

## Open-source release choices

The current release candidate adopts option 2 below: the source code and documentation use the MIT License, while the character and AppIcon are excluded and have no separate reuse license. This separation does not cure unresolved generator-terms or reference-input issues.

1. Confirm sufficient rights and place the character/AppIcon under a stated open-content license.
2. Open-source the code while excluding the character/AppIcon from the code license and label them as separately licensed.
3. Remove the character/AppIcon from the public repository and require users to provide their own asset.

Before a general-public release, the publisher should still record the exact generating product/model/date, preserve the applicable OpenAI terms, and confirm the right to use every reference input.
