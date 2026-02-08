use rustpython_derive::FromArgs;
use rustpython_vm::stdlib::StdlibInitFunc;
use std::borrow::Cow;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

pub(crate) fn get_module_inits() -> impl Iterator<Item = (Cow<'static, str>, StdlibInitFunc)> {
    [
        (
            Cow::Borrowed("_dis"),
            Box::new(dis::make_module) as StdlibInitFunc,
        ),
        (
            Cow::Borrowed("gc"),
            Box::new(gc::make_module) as StdlibInitFunc,
        ),
        (
            Cow::Borrowed("math"),
            Box::new(math::make_module) as StdlibInitFunc,
        ),
    ]
    .into_iter()
}

#[pymodule(name = "dis")]
mod dis {
    use rustpython_vm::{
        PyObjectRef, PyRef, PyResult, TryFromObject, VirtualMachine,
        builtins::{PyCode, PyDictRef, PyStrRef},
        bytecode::CodeFlags,
    };

    #[pyfunction]
    fn dis(obj: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        let co = if let Ok(co) = obj.get_attr("__code__", vm) {
            PyRef::try_from_object(vm, co)?
        } else if let Ok(co_str) = PyStrRef::try_from_object(vm, obj.clone()) {
            vm.compile(
                co_str.as_str(),
                rustpython_vm::compiler::Mode::Exec,
                "<dis>".to_owned(),
            )
            .map_err(|err| vm.new_syntax_error(&err, Some(co_str.as_str())))?
        } else {
            PyRef::try_from_object(vm, obj)?
        };
        disassemble(co)
    }

    #[pyfunction]
    fn disassemble(co: PyRef<PyCode>) -> PyResult<()> {
        print!("{}", &co.code);
        Ok(())
    }

    #[pyattr(name = "COMPILER_FLAG_NAMES")]
    fn compiler_flag_names(vm: &VirtualMachine) -> PyDictRef {
        let dict = vm.ctx.new_dict();
        for (name, flag) in CodeFlags::NAME_MAPPING {
            dict.set_item(
                &*vm.new_pyobj(flag.bits()),
                vm.ctx.new_str(*name).into(),
                vm,
            )
            .unwrap();
        }
        dict
    }
}

#[pymodule]
mod gc {
    use super::*;
    use rustpython_vm::{
        PyObjectRef, PyResult, VirtualMachine,
        function::{OptionalArg, PosArgs},
    };

    static ENABLED: AtomicBool = AtomicBool::new(true);
    static DEBUG: AtomicI32 = AtomicI32::new(0);
    static THRESHOLD: RwLock<(usize, usize, usize)> = RwLock::new((700, 10, 10));

    #[derive(FromArgs)]
    struct CollectArgs {
        #[pyarg(positional, optional)]
        generation: OptionalArg<usize>,
    }

    #[pyfunction]
    fn collect(args: CollectArgs) -> usize {
        let _ = args.generation;
        0
    }

    #[pyfunction]
    fn isenabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    #[pyfunction]
    fn enable() {
        ENABLED.store(true, Ordering::Relaxed);
    }

    #[pyfunction]
    fn disable() {
        ENABLED.store(false, Ordering::Relaxed);
    }

    #[pyfunction]
    fn get_count() -> (usize, usize, usize) {
        (0, 0, 0)
    }

    #[pyfunction]
    fn get_debug() -> i32 {
        DEBUG.load(Ordering::Relaxed)
    }

    #[pyfunction]
    fn get_objects(vm: &VirtualMachine) -> PyObjectRef {
        vm.ctx.new_list(vec![]).into()
    }

    #[pyfunction]
    fn get_referents(_objects: PosArgs<PyObjectRef>, vm: &VirtualMachine) -> PyObjectRef {
        vm.ctx.new_list(vec![]).into()
    }

    #[pyfunction]
    fn get_referrers(_objects: PosArgs<PyObjectRef>, vm: &VirtualMachine) -> PyObjectRef {
        vm.ctx.new_list(vec![]).into()
    }

    #[pyfunction]
    fn get_stats(vm: &VirtualMachine) -> PyObjectRef {
        vm.ctx.new_list(vec![]).into()
    }

    #[pyfunction]
    fn get_threshold() -> (usize, usize, usize) {
        *THRESHOLD.read().expect("gc threshold lock poisoned")
    }

    #[pyfunction]
    fn is_tracked(_obj: PyObjectRef) -> bool {
        true
    }

    #[pyfunction]
    fn set_debug(flags: i32) {
        DEBUG.store(flags, Ordering::Relaxed);
    }

    #[derive(FromArgs)]
    struct SetThresholdArgs {
        #[pyarg(positional)]
        t0: usize,
        #[pyarg(positional, optional)]
        t1: OptionalArg<usize>,
        #[pyarg(positional, optional)]
        t2: OptionalArg<usize>,
    }

    #[pyfunction]
    fn set_threshold(args: SetThresholdArgs, vm: &VirtualMachine) -> PyResult<()> {
        let t0 = args.t0;
        let t1 = args.t1.unwrap_or(10);
        let t2 = args.t2.unwrap_or(10);

        if t0 == 0 {
            return Err(vm.new_value_error("threshold0 must be > 0".to_owned()));
        }

        *THRESHOLD.write().expect("gc threshold lock poisoned") = (t0, t1, t2);
        Ok(())
    }
}

#[pymodule]
mod math {
    use rustpython_vm::{
        PyResult, VirtualMachine,
        function::{ArgIntoFloat, OptionalArg},
    };

    #[pyattr]
    use std::f64::consts::{E as e, PI as pi, TAU as tau};

    #[pyattr(name = "inf")]
    const INF: f64 = f64::INFINITY;

    #[pyattr(name = "nan")]
    const NAN: f64 = f64::NAN;

    #[pyfunction]
    fn fabs(x: ArgIntoFloat) -> f64 {
        x.abs()
    }

    #[pyfunction]
    fn isfinite(x: ArgIntoFloat) -> bool {
        x.is_finite()
    }

    #[pyfunction]
    fn isinf(x: ArgIntoFloat) -> bool {
        x.is_infinite()
    }

    #[pyfunction]
    fn isnan(x: ArgIntoFloat) -> bool {
        x.is_nan()
    }

    #[pyfunction]
    fn copysign(x: ArgIntoFloat, y: ArgIntoFloat) -> f64 {
        x.copysign(*y)
    }

    #[pyfunction]
    fn exp(x: ArgIntoFloat, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        let out = x.exp();
        if !out.is_finite() && x.is_finite() {
            Err(vm.new_overflow_error("math range error".to_owned()))
        } else {
            Ok(out)
        }
    }

    #[pyfunction]
    fn expm1(x: ArgIntoFloat, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        let out = x.exp_m1();
        if !out.is_finite() && x.is_finite() {
            Err(vm.new_overflow_error("math range error".to_owned()))
        } else {
            Ok(out)
        }
    }

    #[pyfunction]
    fn log(x: ArgIntoFloat, base: OptionalArg<ArgIntoFloat>, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        if x.is_sign_negative() || x == 0.0 {
            return Err(vm.new_value_error("math domain error".to_owned()));
        }
        if x.is_nan() {
            return Ok(x);
        }

        let y = x.ln();
        if let OptionalArg::Present(base) = base {
            let b = *base;
            if b <= 0.0 || b == 1.0 || b.is_nan() {
                return Err(vm.new_value_error("math domain error".to_owned()));
            }
            Ok(y / b.ln())
        } else {
            Ok(y)
        }
    }

    #[pyfunction]
    fn log2(x: ArgIntoFloat, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        if x.is_sign_negative() || x == 0.0 {
            return Err(vm.new_value_error("math domain error".to_owned()));
        }
        Ok(x.log2())
    }

    #[pyfunction]
    fn log10(x: ArgIntoFloat, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        if x.is_sign_negative() || x == 0.0 {
            return Err(vm.new_value_error("math domain error".to_owned()));
        }
        Ok(x.log10())
    }

    #[pyfunction]
    fn pow(x: ArgIntoFloat, y: ArgIntoFloat, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        let y = *y;
        if x < 0.0 && y.fract() != 0.0 {
            return Err(vm.new_value_error("math domain error".to_owned()));
        }
        if x == 0.0 && y < 0.0 && y != f64::NEG_INFINITY {
            return Err(vm.new_value_error("math domain error".to_owned()));
        }
        Ok(x.powf(y))
    }

    #[pyfunction]
    fn sqrt(x: ArgIntoFloat, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        if x.is_sign_negative() {
            return Err(vm.new_value_error("math domain error".to_owned()));
        }
        Ok(x.sqrt())
    }

    #[pyfunction]
    fn sin(x: ArgIntoFloat) -> f64 {
        x.sin()
    }

    #[pyfunction]
    fn cos(x: ArgIntoFloat) -> f64 {
        x.cos()
    }

    #[pyfunction]
    fn acos(x: ArgIntoFloat, vm: &VirtualMachine) -> PyResult<f64> {
        let x = *x;
        if (-1.0_f64..=1.0_f64).contains(&x) || x.is_nan() {
            Ok(x.acos())
        } else {
            Err(vm.new_value_error("math domain error".to_owned()))
        }
    }

    #[pyfunction]
    fn ceil(x: ArgIntoFloat) -> i64 {
        x.ceil() as i64
    }

    #[pyfunction]
    fn floor(x: ArgIntoFloat) -> i64 {
        x.floor() as i64
    }

    #[pyfunction]
    fn trunc(x: ArgIntoFloat) -> i64 {
        x.trunc() as i64
    }
}
