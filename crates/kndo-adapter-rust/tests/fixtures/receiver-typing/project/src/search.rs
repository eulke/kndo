use crate::hiargs::HiArgs;

pub(crate) fn run(args: &HiArgs) {
    let _ = args.matcher();
}
