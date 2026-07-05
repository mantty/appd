"""Aspect that harvests the static link inputs for an appd-workerd cc_library.

Links a throwaway dynamic library from the target's CcInfo: it needs no
`main`, and cc_common.link emits a `.params` file with toolchain-correct
link order and alwayslink/whole-archive flags.
"""

load("@rules_cc//cc:find_cc_toolchain.bzl", "find_cc_toolchain", "use_cc_toolchain")
load("@rules_cc//cc/common:cc_common.bzl", "cc_common")
load("@rules_cc//cc/common:cc_info.bzl", "CcInfo")

def _link_inputs_aspect_impl(target, ctx):
    if CcInfo not in target:
        return []

    cc_toolchain = find_cc_toolchain(ctx)
    feature_configuration = cc_common.configure_features(
        ctx = ctx,
        cc_toolchain = cc_toolchain,
        requested_features = ctx.features,
        unsupported_features = ctx.disabled_features,
    )

    linking_context = target[CcInfo].linking_context

    linking_outputs = cc_common.link(
        actions = ctx.actions,
        name = ctx.label.name + "-appd-link-inputs",
        feature_configuration = feature_configuration,
        cc_toolchain = cc_toolchain,
        linking_contexts = [linking_context],
        output_type = "dynamic_library",
    )

    library = linking_outputs.library_to_link
    outputs = [
        file
        for file in [library.dynamic_library, library.interface_library]
        if file != None
    ]

    # A cache-hit link action materializes only its own outputs, not the
    # libraries it links against; request every LinkerInput file so the
    # harvested .params is always backed by files on disk.
    for linker_input in linking_context.linker_inputs.to_list():
        for library_to_link in linker_input.libraries:
            archives = [
                file
                for file in [library_to_link.static_library, library_to_link.pic_static_library]
                if file != None
            ]
            if archives:
                # The link references archives; their loose object lists
                # would materialize the same code twice.
                outputs.extend(archives)
            else:
                outputs.extend(library_to_link.objects or [])
                outputs.extend(library_to_link.pic_objects or [])

    return [OutputGroupInfo(appd_link_inputs = depset(outputs))]

link_inputs_aspect = aspect(
    implementation = _link_inputs_aspect_impl,
    attrs = {},
    fragments = ["cpp"],
    toolchains = use_cc_toolchain(),
)
