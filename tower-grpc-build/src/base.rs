use codegen;
use comments_to_rustdoc;
use prost_build;

/// Generates service code
pub struct ServiceGenerator;

impl ServiceGenerator {
    /// Generate the gRPC server code
    pub fn generate(&self,
                    service: &prost_build::Service,
                    scope: &mut codegen::Scope) {
        self.define(service, scope);
    }

    fn define(&self,
              service: &prost_build::Service,
              scope: &mut codegen::Scope) {
        {
            scope.import("::tower_grpc::codegen::server", "*");

            // Defines the `FooService` and `Foo` traits.
            self.define_service_trait(service, scope);

            // Define the `FooServer` struct
            self.define_server_struct(service, scope);

            // Implement `FooService` for any `FooServer<Foo>`.
            self.define_blanket_impl(service, scope);

            // Define request structs `Get(GetBar)` etc.
            for method in &service.methods {
                let upper_name = ::to_upper_camel(&method.name);
            	scope.new_struct(&upper_name)
            	    .vis("pub")
            	    .derive("Debug")
            	    .derive("Clone")
            	    .tuple_field(&format!("pub {}", method.input_type))
            	    ;
            }
        }
    }

    fn define_service_trait(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {

        // pub trait Foo: Clone { ... 
        let mut service_trait = codegen::Trait::new(&service.name);
        service_trait.vis("pub")
            .parent("Clone")
            .doc(&comments_to_rustdoc(&service.comments))
            ;

        // pub trait FooService: Clone + ... {}
        let mut tower_service_trait = codegen::Trait::new(&format!("{}Service", service.name));
        tower_service_trait.vis("pub")
            .doc("Auto-generated trait for compatibility with `tower::Service`")
            .parent("Clone");


        for method in &service.methods {
            let name = &method.name;
            let upper_name = ::to_upper_camel(&method.proto_name);

            // FooService: Service<Get, .. >
            let tower_bound = format!(
            	"tower_service::Service<{}, Response={}, Error=failure::Error>",
            	upper_name, method.output_type);
            tower_service_trait.parent(&tower_bound);


            let future_bound;
            let output_type = ::unqualified(&method.output_type, &method.output_proto_type, 1);

            if method.server_streaming {
                let stream_name = format!("{}Stream", &upper_name);
                let stream_bound = format!(
                    "futures::Stream<Item = {}, Error = failure::Error> + 'static",
                    output_type);

                future_bound = format!(
                    "'static + futures::Future<Item = Self::{}, Error = failure::Error> + 'static",
                    stream_name);

                // type GetStream: Stream<Item=...>
                service_trait.associated_type(&stream_name)
                    .bound(&stream_bound);
            } else {
                future_bound = format!(
                    "futures::Future<Item = {}, Error = failure::Error> + 'static",
                    output_type);
            }

            let future_name = format!("{}Future", &upper_name);
            // type GetFuture ...
            service_trait.associated_type(&future_name)
                // ... : Future<Item = ... >
                .bound(&future_bound)
                ;

            let input_type = ::unqualified(&method.input_type, &method.input_proto_type, 1);

            // Request type is either a Stream<Item=GetBar> or a straight GetBar
            let request_type = if method.client_streaming {
            	let stream_name = format!("{}Streaming", &upper_name);
            	let stream_bound = format!(
            	    "futures::Stream<Item = {}, Error = failure::Error>",
            	    ::unqualified(&method.input_type, &method.input_proto_type, 1));

            	service_trait.associated_type(&stream_name)
            	    .bound(&stream_bound);
                format!("Self::{}", stream_name)
            } else {
            	input_type
            };

            // fn get(request: GetBar) -> Self::GetFuture
            service_trait.new_fn(&name)
                .arg_mut_self()
                .arg("request", &request_type)
                .ret(&format!("Self::{}Future", &upper_name))
                .doc(&comments_to_rustdoc(&method.comments))
                ;
        }

        scope.push_trait(service_trait);
        scope.push_trait(tower_service_trait);
    }

    fn define_server_struct(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
        let name = format!("{}Server", service.name);
        let lower_name = ::lower_name(&service.name);

        // FooServer<T>
        scope.new_struct(&name)
            .doc(&format!("Auto-generated struct to wrap {}", &service.name))
            .vis("pub")
            .derive("Debug")
            .derive("Clone")
            .generic("T")
            .field(&format!("pub {}", lower_name), "T")
            ;

        // fn new() -> FooServer<T>
        scope.new_impl(&name)
            .generic("T")
            .target_generic("T")
            .bound("T", &service.name)
            .new_fn("new")
            .vis("pub")
            .arg(&lower_name, "T")
            .ret("Self")
            .line(format!("Self {{ {} }}", lower_name))
            ;

        // MakeService impl
        {
            let imp = scope.new_impl(&name)
                .generic("T")
                .target_generic("T")
                .impl_trait("tower::Service<()>")
                .bound("T", &service.name)
                .associate_type("Response", "Self")
                .associate_type("Error", "grpc::Never")
                .associate_type("Future", "futures::FutureResult<Self::Response, Self::Error>")
                ;


            imp.new_fn("poll_ready")
                .arg_mut_self()
                .ret("futures::Poll<(), Self::Error>")
                .line("Ok(futures::Async::Ready(()))")
                ;

            imp.new_fn("call")
                .arg_mut_self()
                .arg("_target", "()")
                .ret("Self::Future")
                .line("futures::ok(self.clone())")
                ;
        }
    }

    fn define_blanket_impl(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
    	let server_name = format!("{}Server", service.name);
        let lower_name = ::lower_name(&service.name);
        for method in &service.methods {
            let upper_name = ::to_upper_camel(&method.name);
            // impl<T: Foo> Service<Get> for FooServer<T> { .. }
    		let imp = scope.new_impl(&server_name)
    		    .generic("T")
    		    .target_generic("T")
    		    .impl_trait(&format!("tower::Service<{}>", &upper_name))
    		    .bound("T", &service.name)
    		    .associate_type("Response", &method.output_type)
    		    .associate_type("Error", "failure::Error")
    		    .associate_type("Future", &format!("T::{}Future", upper_name))
    		    ;

		    imp.new_fn("poll_ready")
		        .arg_mut_self()
		        .ret("futures::Poll<(), Self::Error>")
		        .line("Ok(futures::Async::Ready(()))")
		        ;

		    imp.new_fn("call")
		        .arg_mut_self()
		        .arg("request", &upper_name)
		        .ret("Self::Future")
		        .line(&format!("self.{}.{}(request.0)", lower_name, &method.name))
		        ;
        }

        scope.new_impl(&server_name)
            .generic("T")
            .bound("T", &service.name)
            .target_generic("T")
            .impl_trait(&format!("{}Service", &service.name))
            ;
    }
}

