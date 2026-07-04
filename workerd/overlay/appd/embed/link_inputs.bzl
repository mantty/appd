"""Aspect that harvests the static link inputs for an appd-workerd cc_library.

Links a throwaway dynamic library from the target's own CcInfo rather than
an executable, since an executable needs a `main` symbol nothing here
provides. Dynamic libraries don't need an entry point, and Bazel's own
cc_common.link resolves alwayslink/whole-archive flags exactly the way a
real cc_binary link would, so the resulting `.params` file is link-order-
and flag-correct without this aspect having to hand-roll per-platform
linker syntax.
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

    # cc_common.link() only forces the actions needed for ITS OWN outputs
    # (the dylib/params above) to run. If that link action itself comes back
    # as a cache hit -- disk or remote -- Bazel has no reason to also
    # materialize the real static libraries it links against, since nothing
    # else in this output group asked for them. Request every file any
    # LinkerInput in the closure could contribute, so the harvested .params
    # is always backed by real files on disk regardless of the link action's
    # own cache state.
    for linker_input in linking_context.linker_inputs.to_list():
        for library_to_link in linker_input.libraries:
            archives = [
                file
                for file in [library_to_link.static_library, library_to_link.pic_static_library]
                if file != None
            ]
            if archives:
                # An archive is what real link commands reference here in
                # practice; skip the (often huge, already-archived) loose
                # object lists too so this doesn't force-download every
                # object file of every dependency for no reason.
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
