// @ts-check

import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import starlightOpenAPI, { openAPISidebarGroups } from "starlight-openapi";

// https://astro.build/config
export default defineConfig({
    integrations: [
        starlight({
            plugins: [
                starlightOpenAPI([
                    {
                        base: "api",
                        schema: "./openapi.json",
                    },
                ]),
            ],
            title: "Kennel Docs",
            social: [
                {
                    icon: "codeberg",
                    label: "Codeberg",
                    href: "https://codeberg.org/ScottyLabs/kennel",
                },
            ],
            sidebar: [
                {
                    label: "Architecture",
                    autogenerate: { directory: "architecture" },
                },
                {
                    label: "Guides",
                    items: [
                        { label: "Webhooks", slug: "guides/webhooks" },
                        {
                            label: "Usage",
                            slug: "guides/usage",
                        },
                        {
                            label: "NixOS Deployment",
                            slug: "guides/nixos-deployment",
                        },
                    ],
                },
                {
                    label: "Reference",
                    items: [
                        { label: "kennel.toml", slug: "reference/kennel-toml" },
                    ],
                },
                ...openAPISidebarGroups,
            ],
        }),
    ],
});
