use core::ops::{ Deref, DerefMut };
use bevy_ecs::{
    component::{
        ComponentId,
        Tick
    },
    query::FilteredAccessSet,
    resource::Resource,
    storage::ResourceData,
    system::{
        IntoSystem,
        System,
        BoxedSystem,
        SystemInput,
        SystemParam,
        SystemParamValidationError,
        SystemMeta,
        SystemState,
        PipeSystem,
        CombinatorSystem,
        Combine,
        AdapterSystem,
        Adapt,
        FunctionSystem,
        SystemParamFunction,
        IntoResult as IntoSystemResult,
        ResMut
    },
    world::{
        World,
        unsafe_world_cell::UnsafeWorldCell
    }
};


pub mod prelude {
    pub use crate::{
        AppExt as _,
        Request,
        Req,
        Callback,
        // OptionCallback
    };
}


#[derive(Resource)]
struct ErasedReqSystem<R>
where
    R : Request + 'static
{
    system : BoxedSystem<Req<R>, R::Response>,
    access : FilteredAccessSet
}


#[cfg(feature = "app")]
pub trait AppExt {
    fn add_callback<R, S, M>(&mut self, system : S) -> &mut Self
    where
        R         : Request + 'static,
        S         : IntoSystem<Req<R>, R::Response, M> + 'static,
        S::System : ParametisedSystem;
}
#[cfg(feature = "app")]
impl AppExt for bevy_app::App {
    fn add_callback<R, S, M>(&mut self, system : S) -> &mut Self
    where
        R         : Request + 'static,
        S         : IntoSystem<Req<R>, R::Response, M> + 'static,
        S::System : ParametisedSystem
    {
        let world = self.world_mut();
        if (world.contains_resource::<ErasedReqSystem<R>>()) {
            panic!(
                "Duplicate ({}) -> {} callback registered",
                core::any::type_name::<R>(),
                core::any::type_name::<R::Response>()
            );
        }

        let mut system = IntoSystem::into_system(system);
        let     state  = SystemState::<<S::System as ParametisedSystem>::Param>::new(world);
        let mut meta   = state.meta().clone();
        let mut access = FilteredAccessSet::new();
        <S::System as ParametisedSystem>::Param::init_access(state.param_state(), &mut meta, &mut access, world);
        assert_eq!(state.meta().has_deferred(), meta.has_deferred());
        assert_eq!(state.meta().is_send(),      meta.is_send());
        assert_eq!(state.meta().name(),         meta.name());

        system.initialize(world);
        world.insert_resource(ErasedReqSystem {
            system : Box::new(system),
            access
        });
        self
    }
}


pub trait Request {
    type Response;
}


#[derive(Debug)]
pub struct Req<R>(pub R);

impl<R> Deref for Req<R> {
    type Target = R;
    #[inline]
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl<R> DerefMut for Req<R> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}

impl<R> SystemInput for Req<R>
where
    R : 'static
{
    type Param<'i> = Req<R>;
    type Inner<'i> = Req<R>;
    #[inline]
    fn wrap(this: Self::Inner<'_>) -> Self::Param<'_> { this }
}


pub struct Callback<'w, R>
where
    R : Request + 'static
{
    erased : ResMut<'w, ErasedReqSystem<R>>,
    world  : UnsafeWorldCell<'w>
}

impl<R> Callback<'_, R>
where
    R : Request
{
    pub fn request(&mut self, r : R) -> R::Response {
        unsafe { self.erased.system.run_unsafe(Req(r), self.world) }.unwrap()
    }
}

unsafe impl<R> SystemParam for Callback<'_, R>
where
    R : Request
{
    type State        = ComponentId;
    type Item<'w, 's> = Callback<'w, R>;

    fn init_state(world : &mut World) -> Self::State {
        <ResMut<ErasedReqSystem<R>> as SystemParam>::init_state(world)
    }

    fn init_access(
        component_id         : &Self::State,
        system_meta          : &mut SystemMeta,
        component_access_set : &mut FilteredAccessSet,
        world                : &mut World
    ) {
        let Some(erased) = world.get_resource::<ErasedReqSystem<R>>() else {
            panic!(
                "Callback<{}> requested by system {} is not registered. Consider registering it.",
                core::any::type_name::<R>(),
                system_meta.name()
            );
        };
        if (! component_access_set.is_compatible(&erased.access)) {
            panic!(
                "error[B0002]: A parameter in system {} (via Callback<{}>) conflicts with a previous parameter in system {}. Consider removing the duplicate access. See: https://bevyengine.org/learn/errors/b0002",
                erased.system.name(),
                core::any::type_name::<R>(),
                system_meta.name()
            );
        }
        component_access_set.extend(erased.access.clone());
        <ResMut<ErasedReqSystem<R>> as SystemParam>::init_access(component_id, system_meta, component_access_set, world);
    }

    unsafe fn validate_param(
        &mut component_id : &mut Self::State,
        _system_meta      : &SystemMeta,
        world             : UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        if unsafe { world.storages() }
            .resources
            .get(component_id)
            .is_some_and(ResourceData::is_present)
        {
            Ok(())
        } else {
            Err(SystemParamValidationError::invalid::<Self>(
                "Resource does not exist",
            ))
        }
    }

    unsafe fn get_param<'w, 's>(
        state       : &'s mut Self::State,
        system_meta : &SystemMeta,
        world       : UnsafeWorldCell<'w>,
        change_tick : Tick
    ) -> Self::Item<'w, 's> {
        Callback {
            erased : unsafe { <ResMut<ErasedReqSystem<R>> as SystemParam>::get_param(state, system_meta, world, change_tick) },
            world
        }
    }

    fn apply(
        _state       : &mut Self::State,
        _system_meta : &SystemMeta,
        world        : &mut World
    ) {
        let     world  = world.as_unsafe_world_cell();
        let mut erased = unsafe { world.get_resource_mut::<ErasedReqSystem<R>>() }.unwrap();
        erased.system.apply_deferred(unsafe { world.world_mut() });
    }

}


pub trait ParametisedSystem
where
    Self : System
{
    type Param : SystemParam;
}

impl<A, B> ParametisedSystem for PipeSystem<A, B>
where
            A     : ParametisedSystem,
            B     : ParametisedSystem,
    for<'l> B::In : SystemInput<Inner<'l> = A::Out>
{ type Param = (A::Param, B::Param,); }

impl<A, B, F> ParametisedSystem for CombinatorSystem<F, A, B>
where
    F : Combine<A, B> + 'static,
    A : ParametisedSystem,
    B : ParametisedSystem
{ type Param = (A::Param, B::Param,); }

impl<F, S> ParametisedSystem for AdapterSystem<F, S>
where
    F : Adapt<S>,
    S : ParametisedSystem
{ type Param = S::Param; }

impl<M, Out, F> ParametisedSystem for FunctionSystem<M, Out, F>
where
    M                                  : 'static,
    Out                                : 'static,
    F                                  : SystemParamFunction<M>,
    <F as SystemParamFunction<M>>::Out : IntoSystemResult<Out>
{ type Param = F::Param; }
